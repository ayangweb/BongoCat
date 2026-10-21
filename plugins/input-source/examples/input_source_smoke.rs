//! Run on a logged-in desktop; optionally switch input sources during sampling.
#[path = "../src/input_source.rs"]
mod input_source;
use input_source::InputSourceMonitor;
use std::time::{Duration, Instant};

fn main() {
    let mut monitor =
        InputSourceMonitor::new().expect("create input source monitor on main thread");
    let start = Instant::now();
    let mut previous = String::new();
    while start.elapsed() < Duration::from_secs(3) {
        let source = monitor.snapshot().expect("read desktop input source");
        assert!(!source.name.is_empty());
        if let Some(icon) = &source.icon_rgba {
            assert_eq!(icon.len(), 4096);
        }
        if previous != source.id {
            println!(
                "input source available; native icon: {}",
                source.icon_rgba.is_some()
            );
            previous = source.id;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    drop(monitor);
    let mut reopened = InputSourceMonitor::new().expect("recreate observer after drop");
    assert!(!reopened.snapshot().unwrap().name.is_empty());
    println!("input source observation and shutdown passed");
}
