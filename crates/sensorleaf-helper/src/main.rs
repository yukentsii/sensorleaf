//! sensorleaf-helper: a tiny root service that exposes a fixed set of
//! read-only hardware counters (RAPL energy, AMD energy MSRs, APERF/MPERF)
//! over a Unix socket. It never writes to any MSR, SMU or register: every
//! device file is opened read-only, and the register list below is fixed.
//!
//! Responses are cached for 500 ms, so clients cannot sample the energy
//! counters at a high rate (the reason RAPL is root-only, CVE-2020-8694).

use std::fs::{self, File};
use std::io::Write;
use std::os::fd::FromRawFd;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MSR_RAPL_PWR_UNIT: u64 = 0xC001_0299;
const MSR_CORE_ENERGY: u64 = 0xC001_029A;
const MSR_PKG_ENERGY: u64 = 0xC001_029B;
const MSR_MPERF: u64 = 0xE7;
const MSR_APERF: u64 = 0xE8;
const RAPL_ZONES: [&str; 2] = ["intel-rapl:0", "intel-rapl:0:0"];
const PM_TABLE: &str = "/sys/kernel/ryzen_smu_drv/pm_table";
const PM_VERSION: &str = "/sys/kernel/ryzen_smu_drv/pm_table_version";
const DEFAULT_SOCKET: &str = "/run/sensorleaf/helper.sock";
const IDLE_EXIT: Duration = Duration::from_secs(120);
const MIN_INTERVAL: Duration = Duration::from_millis(500);

struct Msrs {
    files: Vec<(usize, File)>,
}

impl Msrs {
    fn open() -> Self {
        let mut files = Vec::new();
        for e in fs::read_dir("/dev/cpu").into_iter().flatten().flatten() {
            if let Ok(cpu) = e.file_name().to_string_lossy().parse::<usize>() {
                // Read-only open; the msr driver rejects writes on such a handle.
                if let Ok(f) = File::open(e.path().join("msr")) {
                    files.push((cpu, f));
                }
            }
        }
        files.sort_by_key(|x| x.0);
        Msrs { files }
    }
    fn read(f: &File, reg: u64) -> Option<u64> {
        let mut b = [0u8; 8];
        f.read_exact_at(&mut b, reg).ok()?;
        Some(u64::from_le_bytes(b))
    }
}

fn now_ns() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn sample(msrs: &Msrs) -> String {
    let mut out = String::new();
    out.push_str(&format!("time_ns {}\n", now_ns()));
    for z in RAPL_ZONES {
        let base = format!("/sys/class/powercap/{z}");
        if let (Ok(name), Ok(e), Ok(max)) = (
            fs::read_to_string(format!("{base}/name")),
            fs::read_to_string(format!("{base}/energy_uj")),
            fs::read_to_string(format!("{base}/max_energy_range_uj")),
        ) {
            out.push_str(&format!("rapl {} {} {}\n", name.trim(), e.trim(), max.trim()));
        }
    }
    if let Some((_, f0)) = msrs.files.first() {
        if let Some(u) = Msrs::read(f0, MSR_RAPL_PWR_UNIT) {
            out.push_str(&format!("unit {u}\n"));
        }
        if let Some(p) = Msrs::read(f0, MSR_PKG_ENERGY) {
            out.push_str(&format!("pkg_energy {p}\n"));
        }
    }
    for (cpu, f) in &msrs.files {
        let e = Msrs::read(f, MSR_CORE_ENERGY);
        let a = Msrs::read(f, MSR_APERF);
        let m = Msrs::read(f, MSR_MPERF);
        if let (Some(e), Some(a), Some(m)) = (e, a, m) {
            out.push_str(&format!("cpu {cpu} {e} {a} {m}\n"));
        }
    }
    // ryzen_smu PM table (only if that module is loaded): hex-encoded.
    if let Ok(t) = fs::read(PM_TABLE) {
        let ver = fs::read(PM_VERSION)
            .ok()
            .filter(|v| v.len() >= 4)
            .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
            .unwrap_or(0);
        let hex: String = t.iter().map(|b| format!("{b:02x}")).collect();
        out.push_str(&format!("pm_table {ver:#x} {hex}\n"));
    }
    out.push_str("end\n");
    out
}

fn listener() -> UnixListener {
    let pid_ok = std::env::var("LISTEN_PID").ok().and_then(|p| p.parse::<u32>().ok())
        == Some(std::process::id());
    if pid_ok && std::env::var("LISTEN_FDS").as_deref() == Ok("1") {
        // systemd socket activation: the socket is fd 3.
        return unsafe { UnixListener::from_raw_fd(3) };
    }
    let path = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_SOCKET.into());
    let _ = fs::remove_file(&path);
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = fs::create_dir_all(dir);
    }
    let l = UnixListener::bind(&path).expect("bind socket");
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o666));
    l
}

fn main() {
    let start = Instant::now();
    let last = Arc::new(AtomicU64::new(0));
    {
        let last = last.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));
            let idle = start.elapsed().as_secs() - last.load(Ordering::Relaxed);
            if idle > IDLE_EXIT.as_secs() {
                std::process::exit(0);
            }
        });
    }
    let msrs = Msrs::open();
    let cache: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    for conn in listener().incoming() {
        let Ok(mut conn) = conn else { continue };
        last.store(start.elapsed().as_secs(), Ordering::Relaxed);
        let body = {
            let mut c = cache.lock().unwrap();
            match &*c {
                Some((t, s)) if t.elapsed() < MIN_INTERVAL => s.clone(),
                _ => {
                    let s = sample(&msrs);
                    *c = Some((Instant::now(), s.clone()));
                    s
                }
            }
        };
        let _ = conn.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = conn.write_all(body.as_bytes());
    }
}
