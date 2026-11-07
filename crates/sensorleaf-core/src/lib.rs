//! Sensor collection for SensorLeaf.
pub mod collect;
pub mod gpu_metrics;
pub mod helper;
pub mod model;
pub mod report;
pub mod smu;
pub mod sysfs;

pub use collect::Collector;
pub use model::{Device, Kind, Sensor, Snapshot};
