use crate::model::{format_value, MinMax, Snapshot};

/// Plain-text report: the full tree with Value / Min / Max columns.
pub fn text_report(snap: &Snapshot, mm: Option<&MinMax>, header: &str) -> String {
    let mut rows: Vec<(usize, String, String, String, String)> = Vec::new();
    rows.push((0, snap.host.clone(), String::new(), String::new(), String::new()));
    for d in &snap.devices {
        rows.push((1, d.name.clone(), String::new(), String::new(), String::new()));
        for (k, sensors) in d.groups() {
            rows.push((2, k.group_name().into(), String::new(), String::new(), String::new()));
            for s in sensors {
                let (mn, mx) = mm.map(|m| m.get(d, s)).unwrap_or((s.value, s.value));
                rows.push((3, s.label.clone(), s.format(s.value), format_value(mn, s.unit, s.decimals), format_value(mx, s.unit, s.decimals)));
            }
        }
    }
    let w0 = rows.iter().map(|r| r.0 * 2 + r.1.chars().count()).max().unwrap_or(10).max(6) + 2;
    let wv = rows.iter().map(|r| r.2.chars().count().max(r.3.chars().count()).max(r.4.chars().count())).max().unwrap_or(5).max(5) + 2;
    let mut out = String::from(header);
    out.push_str(&format!("{:<w0$}{:>wv$}{:>wv$}{:>wv$}\n", "Sensor", "Value", "Min", "Max"));
    out.push_str(&"-".repeat(w0 + 3 * wv));
    out.push('\n');
    for (lvl, name, v, mn, mx) in rows {
        let label = format!("{}{}", "  ".repeat(lvl), name);
        out.push_str(&format!("{:<w0$}{:>wv$}{:>wv$}{:>wv$}\n", label, v, mn, mx).trim_end());
        out.push('\n');
    }
    out
}
