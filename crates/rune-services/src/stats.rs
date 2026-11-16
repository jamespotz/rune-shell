//! CPU use, memory use and CPU temperature from /proc and /sys, every 2 seconds.

use std::{fs, time::Duration};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Percent of CPU time busy since the last sample.
    pub cpu: u8,
    /// Percent of memory in use (not counting reclaimable cache).
    pub ram: u8,
    /// CPU package temperature in °C, when a known sensor exists.
    pub temp: Option<u8>,
}

pub fn spawn() -> watch::Receiver<Option<Stats>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let sensor = cpu_sensor();
        let mut last = fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|s| cpu_times(&s));
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|s| cpu_times(&s));
            let cpu = match (last, now) {
                (Some(a), Some(b)) => busy_percent(a, b),
                _ => 0,
            };
            last = now;
            let Some(ram) = fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| mem_percent(&s))
            else {
                return; // not Linux, or /proc is missing
            };
            let temp = sensor.as_ref().and_then(|p| {
                let milli: i64 = fs::read_to_string(p).ok()?.trim().parse().ok()?;
                u8::try_from(milli / 1000).ok()
            });
            if tx.send(Some(Stats { cpu, ram, temp })).is_err() {
                return;
            }
        }
    });
    rx
}

/// (idle, total) jiffies from the `cpu` line of /proc/stat.
fn cpu_times(stat: &str) -> Option<(u64, u64)> {
    let nums: Vec<u64> = stat
        .lines()
        .next()?
        .strip_prefix("cpu ")?
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    // user nice system idle iowait irq softirq steal (guest time is already in user)
    let idle = nums.get(3)? + nums.get(4).unwrap_or(&0);
    Some((idle, nums.iter().take(8).sum()))
}

fn busy_percent((idle0, total0): (u64, u64), (idle1, total1): (u64, u64)) -> u8 {
    let total = total1.saturating_sub(total0);
    let busy = total.saturating_sub(idle1.saturating_sub(idle0));
    (busy * 100).checked_div(total).map_or(0, |p| p as u8)
}

fn mem_percent(meminfo: &str) -> Option<u8> {
    let field = |name: &str| -> Option<u64> {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:")?;
    (total > 0).then(|| ((total.saturating_sub(available)) * 100 / total) as u8)
}

/// The CPU package sensor: Intel `coretemp`, AMD `k10temp`/`zenpower`.
fn cpu_sensor() -> Option<std::path::PathBuf> {
    fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .find_map(|d| {
            let name = fs::read_to_string(d.path().join("name")).ok()?;
            matches!(name.trim(), "coretemp" | "k10temp" | "zenpower")
                .then(|| d.path().join("temp1_input"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_files() {
        let a = cpu_times("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3").unwrap();
        assert_eq!(a, (800, 1000));
        let b = cpu_times("cpu  200 0 200 1300 100 0 0 0 0 0\n").unwrap();
        assert_eq!(busy_percent(a, b), 25); // 800 more jiffies, 600 of them idle
        assert_eq!(busy_percent(a, a), 0);
        assert_eq!(cpu_times("intr 1 2 3"), None);
        let mem = "MemTotal:       16000000 kB\nMemFree:  1 kB\nMemAvailable:    4000000 kB\n";
        assert_eq!(mem_percent(mem), Some(75));
        assert_eq!(mem_percent("MemTotal: 0 kB\nMemAvailable: 0 kB"), None);
    }
}
