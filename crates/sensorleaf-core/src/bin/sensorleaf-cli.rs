//! Prints the sensor tree as text. `--watch` refreshes every second.
use sensorleaf_core::model::MinMax;
use sensorleaf_core::{report::text_report, Collector};
use std::time::Duration;

fn main() {
    let watch = std::env::args().any(|a| a == "--watch");
    let mut c = Collector::new();
    let mut mm = MinMax::default();
    c.sample();
    std::thread::sleep(Duration::from_secs(1));
    loop {
        let snap = c.sample();
        mm.update(&snap);
        let hdr = format!("helper: {}\n", if c.helper_ok { "connected" } else { "not running (root-only sensors hidden)" });
        let r = text_report(&snap, Some(&mm), &hdr);
        if watch {
            print!("\x1b[2J\x1b[H{r}");
        } else {
            print!("{r}");
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
