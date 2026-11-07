/// Sensor groups, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Voltage,
    Temperature,
    Power,
    Current,
    Clock,
    Utilization,
    Throughput,
    Fan,
    Capacity,
    Level,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Voltage,
        Kind::Temperature,
        Kind::Power,
        Kind::Current,
        Kind::Clock,
        Kind::Utilization,
        Kind::Throughput,
        Kind::Fan,
        Kind::Capacity,
        Kind::Level,
    ];

    pub fn group_name(self) -> &'static str {
        match self {
            Kind::Voltage => "Voltages",
            Kind::Temperature => "Temperatures",
            Kind::Power => "Powers",
            Kind::Current => "Currents",
            Kind::Clock => "Clocks",
            Kind::Utilization => "Utilization",
            Kind::Throughput => "Throughput",
            Kind::Fan => "Fans",
            Kind::Capacity => "Capacities",
            Kind::Level => "Levels",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sensor {
    pub label: String,
    pub kind: Kind,
    pub unit: &'static str,
    pub decimals: usize,
    pub value: f64,
}

impl Sensor {
    pub fn new(kind: Kind, label: impl Into<String>, value: f64) -> Self {
        let (unit, decimals) = match kind {
            Kind::Voltage => ("V", 3),
            Kind::Temperature => ("°C", 1),
            Kind::Power => ("W", 2),
            Kind::Current => ("A", 3),
            Kind::Clock => ("MHz", 0),
            Kind::Utilization => ("%", 0),
            Kind::Throughput => ("MB/s", 0),
            Kind::Fan => ("RPM", 0),
            Kind::Capacity => ("mWh", 0),
            Kind::Level => ("%", 0),
        };
        Sensor { label: label.into(), kind, unit, decimals, value }
    }

    pub fn with_unit(mut self, unit: &'static str, decimals: usize) -> Self {
        self.unit = unit;
        self.decimals = decimals;
        self
    }

    pub fn format(&self, v: f64) -> String {
        format_value(v, self.unit, self.decimals)
    }
}

pub fn format_value(v: f64, unit: &str, decimals: usize) -> String {
    format!("{:.*} {}", decimals, v, unit)
}

#[derive(Clone, Debug)]
pub struct Device {
    /// Stable key used for min/max tracking.
    pub id: String,
    pub name: String,
    pub sensors: Vec<Sensor>,
}

impl Device {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Device { id: id.into(), name: name.into(), sensors: Vec::new() }
    }
    pub fn push(&mut self, s: Sensor) {
        if s.value.is_finite() {
            self.sensors.push(s);
        }
    }
    pub fn add(&mut self, kind: Kind, label: impl Into<String>, v: Option<f64>) {
        if let Some(v) = v {
            self.push(Sensor::new(kind, label, v));
        }
    }
    pub fn groups(&self) -> Vec<(Kind, Vec<&Sensor>)> {
        Kind::ALL
            .iter()
            .filter_map(|k| {
                let v: Vec<&Sensor> = self.sensors.iter().filter(|s| s.kind == *k).collect();
                (!v.is_empty()).then_some((*k, v))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub host: String,
    pub devices: Vec<Device>,
}

/// Min/max tracking since start (or since the last clear).
#[derive(Default)]
pub struct MinMax {
    map: std::collections::HashMap<String, (f64, f64)>,
}

impl MinMax {
    pub fn key(dev: &Device, s: &Sensor) -> String {
        format!("{}/{:?}/{}", dev.id, s.kind, s.label)
    }
    pub fn update(&mut self, snap: &Snapshot) {
        for d in &snap.devices {
            for s in &d.sensors {
                let e = self.map.entry(Self::key(d, s)).or_insert((s.value, s.value));
                e.0 = e.0.min(s.value);
                e.1 = e.1.max(s.value);
            }
        }
    }
    pub fn get(&self, dev: &Device, s: &Sensor) -> (f64, f64) {
        self.map.get(&Self::key(dev, s)).copied().unwrap_or((s.value, s.value))
    }
    pub fn clear(&mut self) {
        self.map.clear();
    }
}
