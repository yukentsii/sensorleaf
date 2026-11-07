//! SensorLeaf: a hardware sensor tree (Sensor | Value | Min | Max).

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use sensorleaf_core::model::{format_value, Kind, MinMax};
use sensorleaf_core::report::text_report;
use sensorleaf_core::{Collector, Snapshot};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ROW_H: f32 = 19.0;

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Host,
    Device(DevKind),
    Group(Kind),
    None,
}

#[derive(Clone, Copy, PartialEq)]
enum DevKind {
    Board,
    Cpu,
    Gpu,
    Disk,
    Battery,
    Wifi,
    Usb,
}

fn dev_kind(id: &str) -> DevKind {
    match id.split(':').next().unwrap_or("") {
        "cpu" => DevKind::Cpu,
        "gpu" => DevKind::Gpu,
        "nvme" => DevKind::Disk,
        "bat" => DevKind::Battery,
        "wifi" => DevKind::Wifi,
        "usbc" => DevKind::Usb,
        _ => DevKind::Board,
    }
}

struct Row {
    depth: usize,
    key: String,
    label: String,
    icon: Icon,
    has_children: bool,
    value: String,
    min: String,
    max: String,
}

struct Shared {
    snap: Option<Snapshot>,
    helper_ok: bool,
    minmax: MinMax,
}

struct App {
    shared: Arc<Mutex<Shared>>,
    collapsed: HashSet<String>,
    selected: Option<String>,
    status: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let shared = Arc::new(Mutex::new(Shared { snap: None, helper_ok: false, minmax: MinMax::default() }));
        let s2 = shared.clone();
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            let mut c = Collector::new();
            loop {
                let t = Instant::now();
                let snap = c.sample();
                if let Ok(mut s) = s2.lock() {
                    s.minmax.update(&snap);
                    s.snap = Some(snap);
                    s.helper_ok = c.helper_ok;
                }
                ctx.request_repaint();
                std::thread::sleep(Duration::from_secs(1).saturating_sub(t.elapsed()));
            }
        });
        App { shared, collapsed: HashSet::new(), selected: None, status: "Ready".into() }
    }

    fn rows(&self, s: &Shared) -> Vec<Row> {
        let mut out = Vec::new();
        let Some(snap) = &s.snap else { return out };
        let host_key = "host".to_string();
        let host_open = !self.collapsed.contains(&host_key);
        out.push(Row {
            depth: 0,
            key: host_key,
            label: snap.host.clone(),
            icon: Icon::Host,
            has_children: true,
            value: String::new(),
            min: String::new(),
            max: String::new(),
        });
        if !host_open {
            return out;
        }
        for d in &snap.devices {
            let dk = format!("dev/{}", d.id);
            let dev_open = !self.collapsed.contains(&dk);
            out.push(Row {
                depth: 1,
                key: dk.clone(),
                label: d.name.clone(),
                icon: Icon::Device(dev_kind(&d.id)),
                has_children: true,
                value: String::new(),
                min: String::new(),
                max: String::new(),
            });
            if !dev_open {
                continue;
            }
            for (k, sensors) in d.groups() {
                let gk = format!("{dk}/{k:?}");
                let g_open = !self.collapsed.contains(&gk);
                out.push(Row {
                    depth: 2,
                    key: gk.clone(),
                    label: k.group_name().into(),
                    icon: Icon::Group(k),
                    has_children: true,
                    value: String::new(),
                    min: String::new(),
                    max: String::new(),
                });
                if !g_open {
                    continue;
                }
                for sn in sensors {
                    let (mn, mx) = s.minmax.get(d, sn);
                    out.push(Row {
                        depth: 3,
                        key: format!("{gk}/{}", sn.label),
                        label: sn.label.clone(),
                        icon: Icon::None,
                        has_children: false,
                        value: sn.format(sn.value),
                        min: format_value(mn, sn.unit, sn.decimals),
                        max: format_value(mx, sn.unit, sn.decimals),
                    });
                }
            }
        }
        out
    }

    fn save_report(&mut self) {
        let (snap, report) = {
            let s = self.shared.lock().unwrap();
            let Some(snap) = s.snap.clone() else { return };
            let hdr = format!(
                "SensorLeaf {} monitoring data\nDate: {}\nHost: {}\nRoot helper: {}\n\n",
                env!("CARGO_PKG_VERSION"),
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                snap.host,
                if s.helper_ok { "connected" } else { "not running" }
            );
            let r = text_report(&snap, Some(&s.minmax), &hdr);
            (snap, r)
        };
        let name = format!("{}-{}.txt", snap.host, chrono::Local::now().format("%Y%m%d-%H%M%S"));
        if let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter("Text", &["txt"]).save_file() {
            self.status = match std::fs::write(&path, report) {
                Ok(()) => format!("Saved {}", path.display()),
                Err(e) => format!("Could not save: {e}"),
            };
        }
    }
}

fn setup_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |st| {
        let v = &mut st.visuals;
        v.panel_fill = Color32::from_rgb(0x1e, 0x1f, 0x22);
        v.window_fill = Color32::from_rgb(0x26, 0x27, 0x2b);
        v.extreme_bg_color = Color32::from_rgb(0x18, 0x19, 0x1b);
        v.faint_bg_color = Color32::from_rgb(0x24, 0x25, 0x29);
        v.override_text_color = Some(Color32::from_rgb(0xdc, 0xdc, 0xdc));
        v.selection.bg_fill = Color32::from_rgb(0x2f, 0x5a, 0x8f);
        st.spacing.item_spacing = Vec2::new(6.0, 0.0);
        st.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.0));
        st.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.0));
    });
}

fn group_color(k: Kind) -> (Color32, &'static str) {
    match k {
        Kind::Voltage => (Color32::from_rgb(0xe6, 0xb4, 0x22), "V"),
        Kind::Temperature => (Color32::from_rgb(0xe0, 0x4f, 0x3f), "T"),
        Kind::Power => (Color32::from_rgb(0xf0, 0x8a, 0x24), "W"),
        Kind::Current => (Color32::from_rgb(0xc9, 0x6d, 0xd8), "A"),
        Kind::Clock => (Color32::from_rgb(0x3d, 0x8b, 0xe0), "C"),
        Kind::Utilization => (Color32::from_rgb(0x4c, 0xb0, 0x7a), "U"),
        Kind::Throughput => (Color32::from_rgb(0x2e, 0xb8, 0xb8), "B"),
        Kind::Fan => (Color32::from_rgb(0x5f, 0xc4, 0xe8), "F"),
        Kind::Capacity => (Color32::from_rgb(0x8f, 0xa8, 0x3c), "Q"),
        Kind::Level => (Color32::from_rgb(0x7a, 0xc0, 0x4a), "L"),
    }
}

fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon) {
    let c = r.center();
    match icon {
        Icon::None => {}
        Icon::Host => {
            // monitor
            let scr = Rect::from_center_size(c - Vec2::new(0.0, 1.5), Vec2::new(13.0, 9.0));
            p.rect_filled(scr, 1.5, Color32::from_rgb(0x9a, 0xa3, 0xad));
            p.rect_filled(scr.shrink(1.5), 0.5, Color32::from_rgb(0x2f, 0x6f, 0xb8));
            p.rect_filled(Rect::from_center_size(c + Vec2::new(0.0, 5.0), Vec2::new(7.0, 1.5)), 0.0, Color32::from_rgb(0x9a, 0xa3, 0xad));
        }
        Icon::Device(k) => {
            let col = match k {
                DevKind::Cpu => Color32::from_rgb(0x8d, 0x96, 0xa0),
                DevKind::Gpu => Color32::from_rgb(0xc0, 0x45, 0x45),
                DevKind::Board => Color32::from_rgb(0x3f, 0x8f, 0x55),
                DevKind::Disk => Color32::from_rgb(0x6f, 0x7c, 0x8a),
                DevKind::Battery => Color32::from_rgb(0x6c, 0xb0, 0x45),
                DevKind::Wifi => Color32::from_rgb(0x3d, 0x8b, 0xe0),
                DevKind::Usb => Color32::from_rgb(0xa0, 0x80, 0x50),
            };
            match k {
                DevKind::Cpu | DevKind::Gpu | DevKind::Board => {
                    let chip = Rect::from_center_size(c, Vec2::splat(9.0));
                    for i in 0..3 {
                        let o = -3.0 + 3.0 * i as f32;
                        for (a, b) in [
                            (Pos2::new(c.x + o, chip.top() - 2.0), Pos2::new(c.x + o, chip.top())),
                            (Pos2::new(c.x + o, chip.bottom()), Pos2::new(c.x + o, chip.bottom() + 2.0)),
                            (Pos2::new(chip.left() - 2.0, c.y + o), Pos2::new(chip.left(), c.y + o)),
                            (Pos2::new(chip.right(), c.y + o), Pos2::new(chip.right() + 2.0, c.y + o)),
                        ] {
                            p.line_segment([a, b], Stroke::new(1.0, col));
                        }
                    }
                    p.rect_filled(chip, 1.0, col);
                    p.rect_filled(chip.shrink(2.5), 0.5, Color32::from_rgb(0x2a, 0x2b, 0x2f));
                }
                DevKind::Battery => {
                    let b = Rect::from_center_size(c - Vec2::new(1.0, 0.0), Vec2::new(12.0, 7.0));
                    p.rect_stroke(b, 1.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
                    p.rect_filled(Rect::from_min_size(b.min + Vec2::new(2.0, 2.0), Vec2::new(6.0, 3.0)), 0.0, col);
                    p.rect_filled(Rect::from_center_size(Pos2::new(b.right() + 1.0, c.y), Vec2::new(2.0, 3.0)), 0.0, col);
                }
                DevKind::Wifi => {
                    for (i, rad) in [2.0f32, 5.0, 8.0].iter().enumerate() {
                        let base = Pos2::new(c.x, c.y + 4.0);
                        if i == 0 {
                            p.circle_filled(base, 1.6, col);
                        } else {
                            let pts: Vec<Pos2> = (0..=8)
                                .map(|j| {
                                    let a = std::f32::consts::PI * (1.25 + 0.5 * j as f32 / 8.0);
                                    base + Vec2::new(a.cos(), a.sin()) * *rad
                                })
                                .collect();
                            p.add(egui::Shape::line(pts, Stroke::new(1.4, col)));
                        }
                    }
                }
                DevKind::Disk | DevKind::Usb => {
                    let b = Rect::from_center_size(c, Vec2::new(12.0, 8.0));
                    p.rect_filled(b, 1.5, col);
                    p.circle_filled(Pos2::new(b.right() - 2.5, b.bottom() - 2.5), 1.0, Color32::from_rgb(0x9f, 0xe0, 0x7a));
                }
            }
        }
        Icon::Group(k) => {
            let (col, ch) = group_color(k);
            let b = Rect::from_center_size(c, Vec2::splat(12.0));
            p.rect_filled(b, 2.5, col);
            p.text(c, egui::Align2::CENTER_CENTER, ch, FontId::monospace(9.5), Color32::from_rgb(0x15, 0x15, 0x15));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let mut want_save = false;
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Save monitoring data…").clicked() {
                        want_save = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Clear Min/Max").clicked() {
                        self.shared.lock().unwrap().minmax.clear();
                        self.status = "Min/Max cleared".into();
                        ui.close();
                    }
                    if ui.button("Expand all").clicked() {
                        self.collapsed.clear();
                        ui.close();
                    }
                    if ui.button("Collapse groups").clicked() {
                        let s = self.shared.lock().unwrap();
                        if let Some(snap) = &s.snap {
                            for d in &snap.devices {
                                for (k, _) in d.groups() {
                                    self.collapsed.insert(format!("dev/{}/{k:?}", d.id));
                                }
                            }
                        }
                        ui.close();
                    }
                });
                ui.menu_button("Help", |ui| {
                    ui.label(format!("SensorLeaf {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Hardware sensor monitor for Linux.");
                    ui.label("MIT License");
                });
            });
        });
        if want_save {
            self.save_report();
        }
        let helper_ok = self.shared.lock().map(|s| s.helper_ok).unwrap_or(false);
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(if helper_ok { "root helper: connected" } else { "root helper: off" });
                });
            });
            ui.add_space(2.0);
        });

        let bg = ui.visuals().extreme_bg_color;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| {
            let rows = {
                let s = self.shared.lock().unwrap();
                self.rows(&s)
            };
            let mut toggle: Option<String> = None;
            let mut select: Option<String> = None;
            let avail = ui.available_width();
            let vw = 92.0;
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial((avail - 3.0 * vw - 24.0).max(220.0)).at_least(120.0).clip(true))
                .column(Column::initial(vw).at_least(60.0))
                .column(Column::initial(vw).at_least(60.0))
                .column(Column::remainder().at_least(60.0))
                .header(22.0, |mut h| {
                    for t in ["Sensor", "Value", "Min", "Max"] {
                        h.col(|ui| {
                            ui.add_space(4.0);
                            ui.strong(t);
                        });
                    }
                })
                .body(|body| {
                    body.rows(ROW_H, rows.len(), |mut row| {
                        let r = &rows[row.index()];
                        row.set_selected(self.selected.as_deref() == Some(r.key.as_str()));
                        row.col(|ui| {
                            ui.add_space(4.0 + r.depth as f32 * 16.0);
                            let (ar, ares) = ui.allocate_exact_size(Vec2::new(12.0, ROW_H), Sense::click());
                            if r.has_children {
                                let open = !self.collapsed.contains(&r.key);
                                let c = ar.center();
                                let col = if ares.hovered() { Color32::WHITE } else { Color32::from_gray(0xa0) };
                                let pts = if open {
                                    vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(4.0, -2.0), c + Vec2::new(0.0, 3.0)]
                                } else {
                                    vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(3.0, 0.0), c + Vec2::new(-2.0, 4.0)]
                                };
                                ui.painter().add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
                                if ares.clicked() {
                                    toggle = Some(r.key.clone());
                                }
                            }
                            if r.icon != Icon::None {
                                let (ir, _) = ui.allocate_exact_size(Vec2::new(16.0, ROW_H), Sense::hover());
                                paint_icon(ui.painter(), ir, r.icon);
                            }
                            if r.depth < 3 {
                                ui.label(egui::RichText::new(&r.label).strong());
                            } else {
                                ui.label(&r.label);
                            }
                        });
                        row.col(|ui| {
                            ui.label(&r.value);
                        });
                        row.col(|ui| {
                            ui.label(&r.min);
                        });
                        row.col(|ui| {
                            ui.label(&r.max);
                        });
                        let resp = row.response();
                        if resp.clicked() {
                            select = Some(r.key.clone());
                        }
                        if resp.double_clicked() && r.has_children {
                            toggle = Some(r.key.clone());
                        }
                    });
                });
            if let Some(k) = toggle {
                if !self.collapsed.remove(&k) {
                    self.collapsed.insert(k);
                }
            }
            if select.is_some() {
                self.selected = select;
            }
        });
    }
}

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SensorLeaf")
            .with_app_id("sensorleaf")
            .with_inner_size([560.0, 760.0])
            .with_min_inner_size([360.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native("SensorLeaf", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
