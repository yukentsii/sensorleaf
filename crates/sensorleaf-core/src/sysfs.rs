use std::fs;
use std::path::{Path, PathBuf};

pub fn read_str(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

pub fn read_f64(p: impl AsRef<Path>) -> Option<f64> {
    read_str(p)?.parse().ok()
}

/// All hwmon directories with their `name`.
pub fn hwmons() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = fs::read_dir("/sys/class/hwmon")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            Some((read_str(p.join("name"))?, p))
        })
        .collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

pub fn hwmon(name: &str) -> Option<PathBuf> {
    hwmons().into_iter().find(|(n, _)| n == name).map(|(_, p)| p)
}

/// Inputs of one type (e.g. "temp", "in", "fan") as (index, label, raw value).
pub fn hwmon_inputs(dir: &Path, prefix: &str, suffix: &str) -> Vec<(u32, Option<String>, f64)> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix(prefix) else { continue };
        let Some(idx) = rest.strip_suffix(suffix) else { continue };
        let Ok(idx) = idx.parse::<u32>() else { continue };
        if let Some(v) = read_f64(e.path()) {
            let label = read_str(dir.join(format!("{prefix}{idx}_label")));
            out.push((idx, label, v));
        }
    }
    out.sort_by_key(|x| x.0);
    out
}

/// Walk up from a sysfs device until a PCI device (with vendor/device files) is found.
pub fn pci_ids_of(dev: &Path) -> Option<(u16, u16)> {
    let mut p = fs::canonicalize(dev).ok()?;
    for _ in 0..6 {
        if let (Some(v), Some(d)) = (read_str(p.join("vendor")), read_str(p.join("device"))) {
            let v = u16::from_str_radix(v.trim_start_matches("0x"), 16).ok()?;
            let d = u16::from_str_radix(d.trim_start_matches("0x"), 16).ok()?;
            return Some((v, d));
        }
        p = p.parent()?.to_path_buf();
    }
    None
}

/// Look up a device name in the system pci.ids database.
pub fn pci_name(vendor: u16, device: u16) -> Option<String> {
    let db = fs::read_to_string("/usr/share/hwdata/pci.ids").ok()?;
    let vtag = format!("{vendor:04x}  ");
    let dtag = format!("\t{device:04x}  ");
    let mut vname = None;
    for line in db.lines() {
        if vname.is_none() {
            if let Some(n) = line.strip_prefix(&vtag) {
                vname = Some(n.to_string());
            }
        } else if let Some(n) = line.strip_prefix(&dtag) {
            let v = vname.unwrap();
            let v = v.split_whitespace().next().unwrap_or("").to_string();
            return Some(format!("{v} {n}"));
        } else if !line.starts_with('\t') && !line.starts_with('#') && !line.is_empty() {
            break;
        }
    }
    None
}
