//! The system panel's numbers: readable sizes, rates and graph series.

use crate::state::STATS_HISTORY;
use rune_services::stats::Stats;
use std::{collections::VecDeque, time::Duration};

pub const SYSTEM_W: f32 = 400.0;
/// The network graph never scales below this, so idle noise stays flat.
const NET_FLOOR: f32 = 64.0 * 1024.0;

/// One graph: values in 0..=1, oldest first, and how many are filled.
pub type Series = ([f32; STATS_HISTORY], usize);

/// `pick` maps a sample to 0..=1; missing values read as 0.
pub fn series(history: &VecDeque<Stats>, pick: impl Fn(&Stats) -> f32) -> Series {
    let mut values = [0.0; STATS_HISTORY];
    for (slot, sample) in values.iter_mut().zip(history) {
        let value = pick(sample);
        *slot = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    (values, history.len().min(STATS_HISTORY))
}

pub fn percent(value: u8) -> f32 {
    f32::from(value.min(100)) / 100.0
}

/// The largest download rate in the history, for scaling the network graph.
pub fn net_scale(history: &VecDeque<Stats>) -> f32 {
    history
        .iter()
        .map(|s| s.net_down.max(s.net_up) as f32)
        .fold(NET_FLOOR, f32::max)
}

/// KiB as "512 MB", "8.0 GB".
pub fn size(kib: u64) -> String {
    let gib = kib as f64 / (1024.0 * 1024.0);
    if gib >= 1.0 {
        format!("{gib:.1} GB")
    } else {
        format!("{:.0} MB", kib as f64 / 1024.0)
    }
}

/// Bytes per second as "820 B/s", "12 KB/s", "1.5 MB/s".
pub fn rate(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", b / (1024.0 * 1024.0))
    } else if b >= 1024.0 {
        format!("{:.0} KB/s", b / 1024.0)
    } else {
        format!("{bytes} B/s")
    }
}

/// "3d 4h", "2h 05m", "12m".
pub fn uptime(duration: Duration) -> String {
    let minutes = duration.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_services::stats::Usage;

    fn sample(cpu: u8, net_down: u64) -> Stats {
        Stats {
            cpu,
            ram: 0,
            temp: None,
            cpu_name: None,
            threads: 1,
            mhz: None,
            load: [0.0; 3],
            mem: Usage::default(),
            swap: Usage::default(),
            gpu: None,
            net_down,
            net_up: 0,
            uptime: Duration::ZERO,
        }
    }

    #[test]
    fn series_is_bounded_and_clamped() {
        let history: VecDeque<_> = [0, 50, 255].map(|cpu| sample(cpu, 0)).into();
        let (values, len) = series(&history, |s| percent(s.cpu));
        assert_eq!(len, 3);
        assert_eq!(&values[..4], [0.0, 0.5, 1.0, 0.0]);
        assert_eq!(series(&history, |_| f32::NAN).0[0], 0.0);
        let full: VecDeque<_> = (0..STATS_HISTORY + 5).map(|_| sample(1, 0)).collect();
        assert_eq!(series(&full, |s| percent(s.cpu)).1, STATS_HISTORY);
    }

    #[test]
    fn network_scale_has_a_floor() {
        assert_eq!(net_scale(&VecDeque::new()), NET_FLOOR);
        let history: VecDeque<_> = [10, 1_000_000].map(|n| sample(0, n)).into();
        assert_eq!(net_scale(&history), 1_000_000.0);
    }

    #[test]
    fn formats_sizes_rates_and_uptime() {
        assert_eq!(size(512 * 1024), "512 MB");
        assert_eq!(size(8 << 20), "8.0 GB");
        assert_eq!(rate(820), "820 B/s");
        assert_eq!(rate(12 * 1024), "12 KB/s");
        assert_eq!(rate(1_572_864), "1.5 MB/s");
        assert_eq!(uptime(Duration::from_secs(12 * 60)), "12m");
        assert_eq!(uptime(Duration::from_secs(2 * 3600 + 300)), "2h 05m");
        assert_eq!(uptime(Duration::from_secs(3 * 86400 + 4 * 3600)), "3d 4h");
    }
}
