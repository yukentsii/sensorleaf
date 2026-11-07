use crate::gpu_metrics::{ok16, ok32, GpuMetricsV3};
use crate::helper::{self, HelperSample};
use crate::model::{Device, Kind, Sensor, Snapshot};
use crate::sysfs::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Collector {
    host: String,
    cpu_name: String,
    board_name: String,
    gpu_dev: Option<PathBuf>,
    gpu_name: String,
    /// logical cpu -> physical core index (#n, 0-based, in core_id order)
    core_of: BTreeMap<usize, usize>,
    n_cores: usize,
    slot_seen: [bool; 16],
    prev_stat: Option<Vec<(u64, u64)>>,
    prev_helper: Option<HelperSample>,
    /// true when the last sample got data from the root helper
    pub helper_ok: bool,
}

fn cpuinfo_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "CPU".into())
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let model = cpuinfo_model();
        let (cpu_name, igpu) = match model.split_once(" w/ ") {
            Some((c, g)) => (c.trim().to_string(), Some(format!("AMD {}", g.trim()))),
            None => (model.clone(), None),
        };
        let host = read_str("/proc/sys/kernel/hostname").unwrap_or_else(|| "localhost".into());
        let board_name = match read_str("/sys/class/dmi/id/product_name") {
            // "ASUS Zenbook S 16 UM5606WA_UM5606WA" -> "ASUS Zenbook S 16 UM5606WA"
            Some(p) => match p.rsplit_once('_') {
                Some((a, b)) if a.ends_with(b) => a.to_string(),
                _ => p,
            },
            None => "Mainboard".into(),
        };

        // amdgpu device that exposes gpu_metrics
        let gpu_dev = fs::read_dir("/sys/class/drm")
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("device"))
            .find(|p| p.join("gpu_metrics").exists() && read_str(p.join("vendor")).as_deref() == Some("0x1002"));
        let gpu_name = igpu
            .or_else(|| gpu_dev.as_ref().and_then(|p| pci_ids_of(p)).and_then(|(v, d)| pci_name(v, d)))
            .unwrap_or_else(|| "GPU".into());

        // physical core numbering
        let mut cpu_core = BTreeMap::new();
        for e in fs::read_dir("/sys/devices/system/cpu").into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if let Some(i) = n.strip_prefix("cpu").and_then(|i| i.parse::<usize>().ok()) {
                if let Some(c) = read_str(e.path().join("topology/core_id")).and_then(|c| c.parse::<usize>().ok()) {
                    let pkg = read_str(e.path().join("topology/physical_package_id"))
                        .and_then(|c| c.parse::<usize>().ok())
                        .unwrap_or(0);
                    cpu_core.insert(i, (pkg, c));
                }
            }
        }
        let mut ids: Vec<(usize, usize)> = cpu_core.values().copied().collect();
        ids.sort();
        ids.dedup();
        let core_of = cpu_core
            .iter()
            .map(|(cpu, id)| (*cpu, ids.iter().position(|x| x == id).unwrap()))
            .collect();

        Collector {
            host,
            cpu_name,
            board_name,
            gpu_dev,
            gpu_name,
            core_of,
            n_cores: ids.len(),
            slot_seen: [false; 16],
            prev_stat: None,
            prev_helper: None,
            helper_ok: false,
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let gm = self
            .gpu_dev
            .as_ref()
            .and_then(|p| fs::read(p.join("gpu_metrics")).ok())
            .and_then(|d| GpuMetricsV3::parse(&d));
        let h = helper::fetch();
        self.helper_ok = h.is_some();
        let stat = read_proc_stat();

        let mut devices = Vec::new();
        devices.push(self.mainboard(gm.as_ref()));
        devices.push(self.cpu(gm.as_ref(), h.as_ref(), stat.as_ref()));
        if let Some(g) = self.gpu(gm.as_ref()) {
            devices.push(g);
        }
        devices.extend(nvme_devices());
        devices.extend(batteries());
        devices.extend(wifi_devices());
        devices.extend(usbc_devices());
        devices.retain(|d| !d.sensors.is_empty());

        self.prev_stat = stat;
        if h.is_some() {
            self.prev_helper = h;
        }
        Snapshot { host: self.host.clone(), devices }
    }

    fn mainboard(&self, gm: Option<&GpuMetricsV3>) -> Device {
        let mut d = Device::new("board", &self.board_name);
        if let Some(p) = hwmon("acpitz") {
            for (i, _, v) in hwmon_inputs(&p, "temp", "_input") {
                d.add(Kind::Temperature, format!("ACPI Thermal Zone #{i}"), Some(v / 1000.0));
            }
        }
        if let Some(g) = gm {
            d.add(Kind::Temperature, "Skin (SMU)", ok16(g.temperature_skin).map(|v| v / 100.0));
            d.add(Kind::Power, "System (Psys)", ok16(g.sys_power).map(|v| v / 1000.0));
        }
        if let Some(p) = hwmon("asus") {
            for (i, l, v) in hwmon_inputs(&p, "fan", "_input") {
                let l = match l.as_deref() {
                    Some("cpu_fan") => "CPU Fan".to_string(),
                    Some("gpu_fan") => "GPU Fan".to_string(),
                    Some(o) => o.to_string(),
                    None => format!("Fan #{i}"),
                };
                d.add(Kind::Fan, l, Some(v));
            }
        }
        if let Some(p) = hwmon("acpi_fan") {
            for (i, _, v) in hwmon_inputs(&p, "fan", "_input") {
                d.add(Kind::Fan, format!("ACPI Fan #{i}"), Some(v));
            }
        }
        d
    }

    fn cpu(&mut self, gm: Option<&GpuMetricsV3>, h: Option<&HelperSample>, stat: Option<&Vec<(u64, u64)>>) -> Device {
        let mut d = Device::new("cpu", &self.cpu_name);

        // --- SMU telemetry (gpu_metrics) ---
        let mut slots = Vec::new();
        if let Some(g) = gm {
            for i in 0..16 {
                if g.coreclk[i] != 0 && g.coreclk[i] != u16::MAX {
                    self.slot_seen[i] = true;
                }
            }
            slots = (0..16).filter(|i| self.slot_seen[*i]).take(self.n_cores).collect();
        }

        // Voltages (ryzen_smu PM table, when available)
        if let Some((ver, t)) = h.and_then(|h| h.pm_table.as_ref()) {
            crate::smu::add_pm_table(&mut d, *ver, t, self.n_cores);
        }

        // Temperatures
        if let Some(p) = hwmon("k10temp") {
            for (i, l, v) in hwmon_inputs(&p, "temp", "_input") {
                let l = match l.as_deref() {
                    Some("Tctl") => "Package (Tctl)".to_string(),
                    Some("Tdie") => "Package (Tdie)".to_string(),
                    Some(o) => o.to_string(),
                    None => format!("Temp #{i}"),
                };
                d.add(Kind::Temperature, l, Some(v / 1000.0));
            }
        }
        if let Some(g) = gm {
            for (n, i) in slots.iter().enumerate() {
                d.add(Kind::Temperature, format!("Core #{n}"), ok16(g.temperature_core[*i]).map(|v| v / 100.0));
            }
            d.add(Kind::Temperature, "SoC", ok16(g.temperature_soc).map(|v| v / 100.0));
        }

        // Powers
        if let Some(g) = gm {
            d.add(Kind::Power, "Package (SMU)", ok32(g.socket_power).map(|v| v / 1000.0));
            d.add(Kind::Power, "APU (SMU)", ok32(g.apu_power).map(|v| v / 1000.0));
            d.add(Kind::Power, "Cores (SMU)", ok32(g.all_core_power).map(|v| v / 1000.0));
            for (n, i) in slots.iter().enumerate() {
                d.add(Kind::Power, format!("Core #{n} (SMU)"), ok16(g.core_power[*i]).map(|v| v / 1000.0));
            }
            d.add(Kind::Power, "NPU (SMU)", ok16(g.ipu_power).map(|v| v / 1000.0));
            if g.dgpu_power != 0 {
                d.add(Kind::Power, "dGPU (SMU)", ok32(g.dgpu_power).map(|v| v / 1000.0));
            }
            if g.stapm_limit != u16::MAX {
                d.add(Kind::Power, "STAPM Limit", Some(g.stapm_limit as f64 / 1000.0));
            }
            if g.current_stapm_limit != u16::MAX {
                d.add(Kind::Power, "STAPM Limit (current)", Some(g.current_stapm_limit as f64 / 1000.0));
            }
        }
        let mut eff_clocks: BTreeMap<usize, f64> = BTreeMap::new();
        if let (Some(h), Some(p)) = (h, self.prev_helper.as_ref()) {
            let dt = (h.time_ns.saturating_sub(p.time_ns)) as f64 / 1e9;
            if dt > 0.05 {
                // The RAPL "core" zone on AMD only reads core 0, so it is not shown.
                for (zone, label) in [("package-0", "Package (RAPL)")] {
                    if let (Some((e1, max)), Some((e0, _))) = (h.rapl.get(zone), p.rapl.get(zone)) {
                        let de = if e1 >= e0 { e1 - e0 } else { e1 + max - e0 };
                        d.add(Kind::Power, label, Some(de as f64 / 1e6 / dt));
                    }
                }
                if let Some(unit) = h.unit {
                    let joule = 1.0 / (1u64 << ((unit >> 8) & 0x1f)) as f64;
                    let wrap = |a: u64, b: u64| (a.wrapping_sub(b)) & 0xffff_ffff;
                    let mut done = std::collections::BTreeSet::new();
                    for (cpu, (e1, a1, _)) in &h.cpus {
                        let Some((e0, a0, _)) = p.cpus.get(cpu) else { continue };
                        if let Some(core) = self.core_of.get(cpu) {
                            if done.insert(*core) {
                                let w = wrap(*e1, *e0) as f64 * joule / dt;
                                d.add(Kind::Power, format!("Core #{core} (MSR)"), Some(w));
                            }
                        }
                        eff_clocks.insert(*cpu, a1.wrapping_sub(*a0) as f64 / dt / 1e6);
                    }
                }
            }
        }
        // per-core MSR rows sort after the SMU rows; keep order stable
        // Clocks
        if let Some(g) = gm {
            for (n, i) in slots.iter().enumerate() {
                d.add(Kind::Clock, format!("Core #{n}"), ok16(g.coreclk[*i]));
            }
        }
        if !eff_clocks.is_empty() {
            for (cpu, f) in &eff_clocks {
                d.add(Kind::Clock, format!("Thread #{cpu} (effective)"), Some(*f));
            }
        } else {
            for cpu in self.core_of.keys() {
                let f = read_f64(format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"));
                d.add(Kind::Clock, format!("Thread #{cpu}"), f.map(|v| v / 1000.0));
            }
        }
        if let Some(g) = gm {
            d.add(Kind::Clock, "Core Limit", ok16(g.core_maxfreq));
            d.add(Kind::Clock, "Infinity Fabric (FCLK)", ok16(g.fclk));
            d.add(Kind::Clock, "Memory Controller (UCLK)", ok16(g.uclk));
            d.add(Kind::Clock, "SoC (SOCCLK)", ok16(g.socclk));
            d.add(Kind::Clock, "NPU (IPUCLK)", ok16(g.ipuclk));
            d.add(Kind::Clock, "NPU MP (MPIPUCLK)", ok16(g.mpipuclk));
        }

        // Utilization
        if let (Some(s1), Some(s0)) = (stat, self.prev_stat.as_ref()) {
            for (i, ((b1, t1), (b0, t0))) in s1.iter().zip(s0.iter()).enumerate() {
                let dt = t1.saturating_sub(*t0);
                if dt == 0 {
                    continue;
                }
                let u = 100.0 * b1.saturating_sub(*b0) as f64 / dt as f64;
                let l = if i == 0 { "Processor".to_string() } else { format!("Thread #{}", i - 1) };
                d.add(Kind::Utilization, l, Some(u));
            }
        }
        if let Some(g) = gm {
            for (n, i) in slots.iter().enumerate() {
                d.add(Kind::Utilization, format!("Core #{n} (C0)"), ok16(g.core_c0[*i]));
            }
            for (k, v) in g.ipu_activity.iter().enumerate() {
                d.add(Kind::Utilization, format!("NPU Column #{k}"), ok16(*v));
            }
            d.add(Kind::Throughput, "DRAM Read", ok16(g.dram_reads));
            d.add(Kind::Throughput, "DRAM Write", ok16(g.dram_writes));
            d.add(Kind::Throughput, "NPU Read", ok16(g.ipu_reads));
            d.add(Kind::Throughput, "NPU Write", ok16(g.ipu_writes));
        }
        d
    }

    fn gpu(&self, gm: Option<&GpuMetricsV3>) -> Option<Device> {
        let dev = self.gpu_dev.as_ref()?;
        let mut d = Device::new("gpu", &self.gpu_name);
        let hw = fs::read_dir(dev.join("hwmon")).ok()?.flatten().next().map(|e| e.path());
        if let Some(hw) = &hw {
            for (_, l, v) in hwmon_inputs(hw, "in", "_input") {
                if v > 0.0 {
                    d.add(Kind::Voltage, l.unwrap_or_else(|| "Voltage".into()).to_uppercase(), Some(v / 1000.0));
                }
            }
            for (_, l, v) in hwmon_inputs(hw, "temp", "_input") {
                let l = match l.as_deref() {
                    Some("edge") => "GPU (Edge)".to_string(),
                    Some(o) => format!("GPU ({o})"),
                    None => "GPU".into(),
                };
                d.add(Kind::Temperature, l, Some(v / 1000.0));
            }
        }
        if let Some(g) = gm {
            d.add(Kind::Temperature, "GFX (SMU)", ok16(g.temperature_gfx).map(|v| v / 100.0));
        }
        if let Some(hw) = &hw {
            let p = read_f64(hw.join("power1_average")).or_else(|| read_f64(hw.join("power1_input")));
            let l = read_str(hw.join("power1_label")).unwrap_or_else(|| "Power".into());
            d.add(Kind::Power, format!("GPU ({l})"), p.map(|v| v / 1e6));
        }
        if let Some(g) = gm {
            d.add(Kind::Power, "GFX (SMU)", ok32(g.gfx_power).map(|v| v / 1000.0));
        }
        if let Some(hw) = &hw {
            for (_, l, v) in hwmon_inputs(hw, "freq", "_input") {
                let l = match l.as_deref() {
                    Some("sclk") => "Graphics (SCLK)".to_string(),
                    Some(o) => o.to_uppercase(),
                    None => "Clock".into(),
                };
                d.add(Kind::Clock, l, Some(v / 1e6));
            }
        }
        if let Some(g) = gm {
            d.add(Kind::Clock, "Graphics (SMU avg)", ok16(g.gfxclk));
            d.add(Kind::Clock, "Graphics Limit", ok16(g.gfx_maxfreq));
        }
        d.add(Kind::Clock, "Memory (MCLK)", dpm_active(&dev.join("pp_dpm_mclk")));
        if let Some(g) = gm {
            d.add(Kind::Clock, "Video (VCLK)", ok16(g.vclk));
            d.add(Kind::Clock, "Video Processing (VPECLK)", ok16(g.vpeclk));
        }
        d.add(Kind::Utilization, "GPU", read_f64(dev.join("gpu_busy_percent")));
        if let Some(g) = gm {
            d.add(Kind::Utilization, "GFX (SMU)", ok16(g.gfx_activity));
            d.add(Kind::Utilization, "Video Engine (VCN)", ok16(g.vcn_activity));
        }
        let mb = |f: &str| read_f64(dev.join(f)).map(|v| v / 1048576.0);
        for (used, total, name) in [("mem_info_vram_used", "mem_info_vram_total", "VRAM"), ("mem_info_gtt_used", "mem_info_gtt_total", "GTT")] {
            if let (Some(u), Some(t)) = (mb(used), mb(total)) {
                if t > 0.0 {
                    d.add(Kind::Utilization, format!("{name} Usage"), Some(100.0 * u / t));
                }
                d.push(Sensor::new(Kind::Capacity, format!("{name} Used"), u).with_unit("MB", 0));
                d.push(Sensor::new(Kind::Capacity, format!("{name} Total"), t).with_unit("MB", 0));
            }
        }
        Some(d)
    }
}

fn dpm_active(p: &Path) -> Option<f64> {
    let s = fs::read_to_string(p).ok()?;
    let l = s.lines().find(|l| l.trim_end().ends_with('*'))?;
    let v = l.split(':').nth(1)?.trim().trim_end_matches('*').trim();
    v.to_lowercase().trim_end_matches("mhz").trim().parse().ok()
}

/// Returns (busy, total) jiffies for "cpu" and each "cpuN" line.
fn read_proc_stat() -> Option<Vec<(u64, u64)>> {
    let s = fs::read_to_string("/proc/stat").ok()?;
    Some(
        s.lines()
            .filter(|l| l.starts_with("cpu"))
            .map(|l| {
                let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
                // user nice system idle iowait irq softirq steal (guest is inside user)
                let total: u64 = v.iter().take(8).sum();
                let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
                (total - idle, total)
            })
            .collect(),
    )
}

fn nvme_devices() -> Vec<Device> {
    let mut out = Vec::new();
    for (name, p) in hwmons() {
        if name != "nvme" {
            continue;
        }
        let model = read_str(p.join("device/model")).unwrap_or_else(|| "NVMe SSD".into());
        let id = fs::canonicalize(p.join("device")).map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let mut d = Device::new(format!("nvme:{id}"), model);
        for (i, l, v) in hwmon_inputs(&p, "temp", "_input") {
            d.add(Kind::Temperature, l.unwrap_or_else(|| format!("Temp #{i}")), Some(v / 1000.0));
        }
        out.push(d);
    }
    out
}

fn batteries() -> Vec<Device> {
    let mut out = Vec::new();
    for e in fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten() {
        let p = e.path();
        if read_str(p.join("type")).as_deref() != Some("Battery") {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let model = read_str(p.join("model_name")).unwrap_or_else(|| "Battery".into());
        let mut d = Device::new(format!("bat:{name}"), format!("{model} ({name})"));
        let f = |k: &str| read_f64(p.join(k));
        let volt = f("voltage_now").map(|v| v / 1e6);
        d.add(Kind::Voltage, "Voltage", volt);
        let status = read_str(p.join("status")).unwrap_or_default();
        // energy_* (µWh) or charge_* (µAh, converted with design voltage)
        let vmin = f("voltage_min_design").map(|v| v / 1e6);
        let cap = |e: &str, c: &str| f(e).map(|v| v / 1000.0).or_else(|| Some(f(c)? / 1000.0 * vmin?));
        let power = f("power_now").map(|v| v / 1e6).or_else(|| Some(f("current_now")? / 1e6 * volt?));
        if let Some(pw) = power {
            let l = if status == "Charging" { "Charge Rate" } else { "Discharge Rate" };
            d.add(Kind::Power, l, Some(pw.abs()));
        }
        if let Some(c) = f("current_now") {
            d.add(Kind::Current, "Current", Some(c / 1e6));
        }
        let design = cap("energy_full_design", "charge_full_design");
        let full = cap("energy_full", "charge_full");
        let now = cap("energy_now", "charge_now");
        d.add(Kind::Capacity, "Designed Capacity", design);
        d.add(Kind::Capacity, "Full Charge Capacity", full);
        d.add(Kind::Capacity, "Current Capacity", now);
        if let (Some(dz), Some(fu)) = (design, full) {
            if dz > 0.0 {
                d.add(Kind::Level, "Wear Level", Some(100.0 * (1.0 - fu / dz)));
            }
        }
        d.add(Kind::Level, "Charge Level", f("capacity"));
        out.push(d);
    }
    out
}

fn wifi_devices() -> Vec<Device> {
    let mut out = Vec::new();
    for (name, p) in hwmons() {
        if !(name.starts_with("mt79") || name.starts_with("iwlwifi") || name.starts_with("ath") || name.contains("phy")) {
            continue;
        }
        let dev_name = pci_ids_of(&p.join("device"))
            .and_then(|(v, dd)| pci_name_cached(v, dd))
            .map(|n| n.split(" [").next().unwrap_or(&n).to_string())
            .unwrap_or_else(|| name.clone());
        let mut d = Device::new(format!("wifi:{name}"), dev_name);
        for (i, l, v) in hwmon_inputs(&p, "temp", "_input") {
            d.add(Kind::Temperature, l.unwrap_or_else(|| if i == 1 { "Temperature".into() } else { format!("Temp #{i}") }), Some(v / 1000.0));
        }
        out.push(d);
    }
    out
}

fn usbc_devices() -> Vec<Device> {
    let mut out = Vec::new();
    let mut n = 0;
    for (name, p) in hwmons() {
        if !name.starts_with("ucsi_source_psy") {
            continue;
        }
        n += 1;
        let mut d = Device::new(format!("usbc:{name}"), format!("USB-C Port {n}"));
        for (_, _, v) in hwmon_inputs(&p, "in", "_input") {
            d.add(Kind::Voltage, "VBUS", Some(v / 1000.0));
        }
        for (_, _, v) in hwmon_inputs(&p, "curr", "_input") {
            d.add(Kind::Current, "Current", Some(v / 1000.0));
        }
        out.push(d);
    }
    out
}

fn pci_name_cached(v: u16, d: u16) -> Option<String> {
    use std::sync::Mutex;
    static CACHE: Mutex<Vec<((u16, u16), Option<String>)>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().ok()?;
    if let Some((_, n)) = c.iter().find(|(k, _)| *k == (v, d)) {
        return n.clone();
    }
    let n = pci_name(v, d);
    c.push(((v, d), n.clone()));
    n
}
