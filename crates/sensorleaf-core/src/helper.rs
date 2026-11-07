//! Client for sensorleaf-helper (see crates/sensorleaf-helper).
use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub const SOCKET: &str = "/run/sensorleaf/helper.sock";

#[derive(Clone, Debug, Default)]
pub struct HelperSample {
    pub time_ns: u128,
    /// zone name -> (energy µJ, max range µJ)
    pub rapl: BTreeMap<String, (u64, u64)>,
    pub unit: Option<u64>,
    pub pkg_energy: Option<u64>,
    /// logical cpu -> (core energy raw, aperf, mperf)
    pub cpus: BTreeMap<usize, (u64, u64, u64)>,
    pub pm_table: Option<(u32, Vec<u8>)>,
}

pub fn fetch() -> Option<HelperSample> {
    let path = std::env::var("SENSORLEAF_HELPER").unwrap_or_else(|_| SOCKET.into());
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    if !buf.ends_with("end\n") {
        return None;
    }
    let mut h = HelperSample::default();
    for line in buf.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            ["time_ns", t] => h.time_ns = t.parse().ok()?,
            ["rapl", name, e, m] => {
                h.rapl.insert(name.to_string(), (e.parse().ok()?, m.parse().ok()?));
            }
            ["unit", u] => h.unit = u.parse().ok(),
            ["pkg_energy", p] => h.pkg_energy = p.parse().ok(),
            ["cpu", c, e, a, m] => {
                h.cpus.insert(c.parse().ok()?, (e.parse().ok()?, a.parse().ok()?, m.parse().ok()?));
            }
            ["pm_table", v, hex] => {
                let ver = u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()?;
                let bytes = (0..hex.len() / 2)
                    .filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
                    .collect();
                h.pm_table = Some((ver, bytes));
            }
            _ => {}
        }
    }
    Some(h)
}
