//! Wall clock that ticks on every second boundary.

use chrono::{DateTime, Local};
use std::time::Duration;
use tokio::sync::watch;

/// Spawns the clock task. The UI formats the time itself and only redraws when
/// the formatted string changes, so a 1 s tick costs nothing visible.
pub fn spawn() -> watch::Receiver<DateTime<Local>> {
    let (tx, rx) = watch::channel(Local::now());
    tokio::spawn(async move {
        loop {
            let ms = 1000 - Local::now().timestamp_subsec_millis().min(999);
            tokio::time::sleep(Duration::from_millis(u64::from(ms))).await;
            if tx.send(Local::now()).is_err() {
                return; // every receiver is gone
            }
        }
    });
    rx
}
