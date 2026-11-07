//! ryzen_smu PM table decoding. Only table versions whose layout was checked
//! on real hardware are decoded; any other version produces no sensors, so
//! values are never guessed.
//!
//! Strix Point, table 0x5D0009: per-core voltages are 12 little-endian f32
//! values at 0xA0C, in core order (Zen 5 cores, then Zen 5c cores). Checked by
//! loading only the Zen 5c cores and watching just those entries rise. This is
//! 4 bytes after the offset RyzenAdj uses (0xA08); on this firmware 0xA08 is
//! not a core voltage.

use crate::model::{Device, Kind};

fn f32_at(t: &[u8], off: usize) -> Option<f64> {
    let b = t.get(off..off + 4)?;
    let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64;
    v.is_finite().then_some(v)
}

pub fn add_pm_table(cpu: &mut Device, ver: u32, table: &[u8], n_cores: usize) {
    let (base, max_cores) = match ver {
        0x005D_0009 => (0xA0C, 12),
        _ => return,
    };
    for n in 0..n_cores.min(max_cores) {
        // Out-of-range values mean the layout does not match: show nothing.
        if let Some(v) = f32_at(table, base + n * 4).filter(|v| (0.2..=1.6).contains(v)) {
            cpu.add(Kind::Voltage, format!("Core #{n}"), Some(v));
        }
    }
}
