//! Hardware stats from /proc, /sys and `nvidia-smi`, every [`INTERVAL`].

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{fs, process::Command, sync::watch};
use tracing::warn;

pub const INTERVAL: Duration = Duration::from_secs(2);
const GPU_RETRY: Duration = Duration::from_secs(30);
const GPU_TIMEOUT: Duration = Duration::from_secs(1);
const NVIDIA_QUERY: &str =
    "--query-gpu=name,utilization.gpu,temperature.gpu,memory.used,memory.total,power.draw";

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    /// Percent of CPU time busy since the last sample.
    pub cpu: u8,
    /// Percent of memory in use (not counting reclaimable cache).
    pub ram: u8,
    /// CPU package temperature in °C, when a known sensor exists.
    pub temp: Option<u8>,
    pub cpu_name: Option<Arc<str>>,
    pub threads: u16,
    /// Mean clock of all threads.
    pub mhz: Option<u32>,
    /// 1, 5 and 15 minute load averages.
    pub load: [f32; 3],
    pub mem: Usage,
    pub swap: Usage,
    pub gpu: Option<Gpu>,
    /// Bytes per second received and sent, over all interfaces but loopback.
    pub net_down: u64,
    pub net_up: u64,
    pub uptime: Duration,
}

/// Used and total, in KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub used: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: Option<Arc<str>>,
    pub busy: u8,
    pub temp: Option<u8>,
    /// Video memory in KiB.
    pub vram: Option<Usage>,
    pub watts: Option<f32>,
}

#[derive(Debug, Clone)]
enum GpuSource {
    Nvidia,
    /// An amdgpu `device` directory and its hwmon temperature file.
    Amd(PathBuf, Option<PathBuf>),
}

struct Counters {
    cpu: Option<(u64, u64)>,
    net: Option<(u64, u64)>,
    at: Instant,
}

impl Counters {
    async fn read() -> Self {
        Self {
            cpu: read("/proc/stat").await.as_deref().and_then(cpu_times),
            net: read("/proc/net/dev").await.as_deref().and_then(net_bytes),
            at: Instant::now(),
        }
    }
}

pub fn spawn() -> watch::Receiver<Option<Stats>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let sensor = cpu_sensor().await;
        let mut gpu_source = gpu_source().await;
        let mut gpu_retry: Option<Instant> = None;
        let cpu_name = read("/proc/cpuinfo")
            .await
            .as_deref()
            .and_then(cpu_model)
            .map(Arc::from);
        let mut last = Counters::read().await;
        loop {
            tokio::time::sleep(INTERVAL).await;
            let now = Counters::read().await;
            let secs = now.at.duration_since(last.at).as_secs_f64();
            let cpu = match (last.cpu, now.cpu) {
                (Some(a), Some(b)) => busy_percent(a, b),
                _ => 0,
            };
            let (net_down, net_up) = match (last.net, now.net) {
                (Some(a), Some(b)) => (rate(a.0, b.0, secs), rate(a.1, b.1, secs)),
                _ => (0, 0),
            };
            last = now;
            let Some((mem, swap)) = read("/proc/meminfo").await.as_deref().and_then(memory) else {
                warn!("stats: /proc/meminfo is unreadable; stopping");
                return;
            };
            let temp = match &sensor {
                Some(path) => read_celsius(path).await,
                None => None,
            };
            let cpuinfo = read("/proc/cpuinfo").await.unwrap_or_default();
            let (threads, mhz) = cpu_clock(&cpuinfo);
            let gpu = match &gpu_source {
                Some(_) if gpu_retry.is_some_and(|at| Instant::now() < at) => None,
                Some(GpuSource::Nvidia) => match nvidia().await {
                    Ok(gpu) => Some(gpu),
                    Err(e) => {
                        warn!("stats: nvidia-smi: {e}");
                        gpu_retry = Instant::now().checked_add(GPU_RETRY);
                        if gpu_retry.is_none() {
                            gpu_source = None;
                        }
                        None
                    }
                },
                Some(GpuSource::Amd(dir, temp)) => amd(dir, temp.as_deref()).await,
                None => None,
            };
            let stats = Stats {
                cpu,
                ram: percent(mem),
                temp,
                cpu_name: cpu_name.clone(),
                threads,
                mhz,
                load: read("/proc/loadavg")
                    .await
                    .as_deref()
                    .map(load_average)
                    .unwrap_or_default(),
                mem,
                swap,
                gpu,
                net_down,
                net_up,
                uptime: read("/proc/uptime")
                    .await
                    .as_deref()
                    .and_then(uptime)
                    .unwrap_or_default(),
            };
            if tx.send(Some(stats)).is_err() {
                return;
            }
        }
    });
    rx
}

async fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).await.ok()
}

async fn read_celsius(path: &Path) -> Option<u8> {
    let milli: i64 = read(path).await?.trim().parse().ok()?;
    u8::try_from(milli / 1000).ok()
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
    busy.saturating_mul(100)
        .checked_div(total)
        .and_then(|p| u8::try_from(p).ok())
        .unwrap_or(0)
}

/// Total (received, sent) bytes over every interface but loopback.
fn net_bytes(dev: &str) -> Option<(u64, u64)> {
    let mut sum = (0u64, 0u64);
    for line in dev.lines().skip(2) {
        let (name, fields) = line.split_once(':')?;
        if name.trim() == "lo" {
            continue;
        }
        let mut fields = fields.split_whitespace().map(|n| n.parse::<u64>().ok());
        let rx = fields.next()??;
        let tx = fields.nth(7)??;
        sum = (sum.0.saturating_add(rx), sum.1.saturating_add(tx));
    }
    Some(sum)
}

fn rate(before: u64, after: u64, secs: f64) -> u64 {
    if secs <= 0. {
        return 0;
    }
    // f64 → u64 `as` saturates; the delta is far below f64's exact range.
    (after.saturating_sub(before) as f64 / secs) as u64
}

fn percent(usage: Usage) -> u8 {
    usage
        .used
        .saturating_mul(100)
        .checked_div(usage.total)
        .and_then(|p| u8::try_from(p).ok())
        .unwrap_or(0)
}

/// (memory, swap) from /proc/meminfo.
fn memory(meminfo: &str) -> Option<(Usage, Usage)> {
    let field = |name: &str| -> Option<u64> {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let total = field("MemTotal:").filter(|&t| t > 0)?;
    let mem = Usage {
        used: total.saturating_sub(field("MemAvailable:")?),
        total,
    };
    let swap_total = field("SwapTotal:").unwrap_or(0);
    let swap = Usage {
        used: swap_total.saturating_sub(field("SwapFree:").unwrap_or(swap_total)),
        total: swap_total,
    };
    Some((mem, swap))
}

fn cpu_model(cpuinfo: &str) -> Option<&str> {
    cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("model name")?.split_once(':'))
        .map(|(_, name)| name.trim())
        .filter(|name| !name.is_empty())
}

/// (thread count, mean MHz) from /proc/cpuinfo.
fn cpu_clock(cpuinfo: &str) -> (u16, Option<u32>) {
    let (mut threads, mut sum, mut clocked) = (0u16, 0f64, 0u32);
    for line in cpuinfo.lines() {
        if line.starts_with("processor") {
            threads = threads.saturating_add(1);
        } else if let Some((_, mhz)) = line
            .strip_prefix("cpu MHz")
            .and_then(|rest| rest.split_once(':'))
            && let Ok(mhz) = mhz.trim().parse::<f64>()
            && mhz.is_finite()
        {
            sum += mhz;
            clocked = clocked.saturating_add(1);
        }
    }
    // f64 → u32 `as` saturates.
    let mean = (clocked > 0).then(|| (sum / f64::from(clocked)) as u32);
    (threads, mean)
}

fn load_average(loadavg: &str) -> [f32; 3] {
    let mut out = [0.; 3];
    for (slot, value) in out.iter_mut().zip(loadavg.split_whitespace()) {
        *slot = value
            .parse()
            .ok()
            .filter(|v: &f32| v.is_finite())
            .unwrap_or(0.);
    }
    out
}

fn uptime(text: &str) -> Option<Duration> {
    let secs: f64 = text.split_whitespace().next()?.parse().ok()?;
    Duration::try_from_secs_f64(secs).ok()
}

/// The CPU package sensor: Intel `coretemp`, AMD `k10temp`/`zenpower`.
async fn cpu_sensor() -> Option<PathBuf> {
    let dir = hwmon_named(&["coretemp", "k10temp", "zenpower"]).await?;
    Some(dir.join("temp1_input"))
}

async fn hwmon_named(names: &[&str]) -> Option<PathBuf> {
    let mut dirs = fs::read_dir("/sys/class/hwmon").await.ok()?;
    while let Ok(Some(dir)) = dirs.next_entry().await {
        let name = read(dir.path().join("name")).await.unwrap_or_default();
        if names.contains(&name.trim()) {
            return Some(dir.path());
        }
    }
    None
}

/// The first DRM card from NVIDIA (vendor 0x10de) or with amdgpu's busy counter.
async fn gpu_source() -> Option<GpuSource> {
    let mut cards = fs::read_dir("/sys/class/drm").await.ok()?;
    while let Ok(Some(card)) = cards.next_entry().await {
        let name = card.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = card.path().join("device");
        let vendor = read(device.join("vendor")).await.unwrap_or_default();
        if vendor.trim() == "0x10de" {
            return Some(GpuSource::Nvidia);
        }
        if fs::metadata(device.join("gpu_busy_percent")).await.is_ok() {
            let temp = first_hwmon(&device.join("hwmon"))
                .await
                .map(|dir| dir.join("temp1_input"));
            return Some(GpuSource::Amd(device, temp));
        }
    }
    None
}

async fn first_hwmon(dir: &Path) -> Option<PathBuf> {
    let mut entries = fs::read_dir(dir).await.ok()?;
    Some(entries.next_entry().await.ok()??.path())
}

async fn amd(device: &Path, temp: Option<&Path>) -> Option<Gpu> {
    let number = |file: &'static str| async move {
        read(device.join(file)).await?.trim().parse::<u64>().ok()
    };
    let busy = u8::try_from(number("gpu_busy_percent").await?).ok()?;
    let vram = match (
        number("mem_info_vram_used").await,
        number("mem_info_vram_total").await,
    ) {
        (Some(used), Some(total)) => Some(Usage {
            used: used / 1024,
            total: total / 1024,
        }),
        _ => None,
    };
    Some(Gpu {
        name: None,
        busy: busy.min(100),
        temp: match temp {
            Some(path) => read_celsius(path).await,
            None => None,
        },
        vram,
        watts: None,
    })
}

// shortcut: spawns nvidia-smi each tick (~20 ms CPU per 2 s); stream `-lms` or bind NVML if it shows up in profiles.
async fn nvidia() -> Result<Gpu, String> {
    let output = tokio::time::timeout(
        GPU_TIMEOUT,
        Command::new("nvidia-smi")
            .args([NVIDIA_QUERY, "--format=csv,noheader,nounits"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| format!("timed out after {GPU_TIMEOUT:?}"))?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("exited with {}", output.status));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_nvidia(&text).ok_or_else(|| format!("unexpected output {:?}", truncate(&text)))
}

fn truncate(text: &str) -> &str {
    text.char_indices()
        .nth(120)
        .map_or(text, |(i, _)| &text[..i])
}

/// The first GPU's line of `nvidia-smi --query-gpu=… --format=csv,noheader,nounits`.
fn parse_nvidia(text: &str) -> Option<Gpu> {
    let line = text.lines().next()?;
    let fields: Vec<&str> = line.splitn(7, ',').map(str::trim).collect();
    let [name, busy, temp, used, total, watts] = fields.as_slice() else {
        return None;
    };
    // Unsupported fields read "[N/A]" or "[Not Supported]".
    let number = |field: &str| {
        field
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.)
    };
    let vram = match (number(used), number(total)) {
        // MiB → KiB; `as` saturates.
        (Some(used), Some(total)) => Some(Usage {
            used: (used * 1024.) as u64,
            total: (total * 1024.) as u64,
        }),
        _ => None,
    };
    Some(Gpu {
        name: (!name.is_empty()).then(|| Arc::from(truncate(name))),
        busy: number(busy).map_or(0, |b| b.min(100.) as u8),
        temp: number(temp).map(|t| t.min(255.) as u8),
        vram,
        watts: number(watts).map(|w| w as f32),
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
        assert_eq!(busy_percent(b, a), 0); // counters going backwards
        assert_eq!(cpu_times("intr 1 2 3"), None);
        let mem = "MemTotal:       16000000 kB\nMemFree:  1 kB\nMemAvailable:    4000000 kB\n\
                   SwapTotal: 8000 kB\nSwapFree: 2000 kB\n";
        let (mem, swap) = memory(mem).unwrap();
        assert_eq!(percent(mem), 75);
        assert_eq!(
            swap,
            Usage {
                used: 6000,
                total: 8000
            }
        );
        assert_eq!(memory("MemTotal: 0 kB\nMemAvailable: 0 kB"), None);
        assert_eq!(percent(Usage::default()), 0);
    }

    #[test]
    fn parses_cpu_and_system_info() {
        let info = "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i5\ncpu MHz\t\t: 1000.0\n\n\
                    processor\t: 1\nmodel name\t: Intel(R) Core(TM) i5\ncpu MHz\t\t: 3000.5\n";
        assert_eq!(cpu_model(info), Some("Intel(R) Core(TM) i5"));
        assert_eq!(cpu_clock(info), (2, Some(2000)));
        assert_eq!(cpu_clock("processor : 0\ncpu MHz : NaN\n"), (1, None));
        assert_eq!(cpu_model("model name :   \n"), None);
        assert_eq!(
            load_average("0.57 0.41 0.23 3/1413 9450"),
            [0.57, 0.41, 0.23]
        );
        assert_eq!(load_average("inf"), [0., 0., 0.]);
        assert_eq!(uptime("12.5 40.0"), Some(Duration::from_millis(12_500)));
        assert_eq!(uptime("-1 0"), None);
    }

    #[test]
    fn network_rate_skips_loopback() {
        let dev = "Inter-|   Receive\n face |bytes\n\
                   lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n\
                   enp4s0: 1000 1 0 0 0 0 0 0 500 1 0 0 0 0 0 0\n\
                   wlan0: 24 1 0 0 0 0 0 0 12 1 0 0 0 0 0 0\n";
        assert_eq!(net_bytes(dev), Some((1024, 512)));
        assert_eq!(net_bytes("a\nb\nbroken line\n"), None);
        assert_eq!(net_bytes("a\nb\neth0: x\n"), None);
        assert_eq!(rate(1000, 3000, 2.), 1000);
        assert_eq!(rate(3000, 1000, 2.), 0);
        assert_eq!(rate(0, 10, 0.), 0);
    }

    #[test]
    fn parses_nvidia_smi_and_rejects_hostile_values() {
        let gpu = parse_nvidia("NVIDIA GeForce RTX 3060, 27, 51.41, 1494, 12288, 53.76\n").unwrap();
        assert_eq!(gpu.name.as_deref(), Some("NVIDIA GeForce RTX 3060"));
        assert_eq!((gpu.busy, gpu.temp), (27, Some(51)));
        assert_eq!(
            gpu.vram,
            Some(Usage {
                used: 1494 * 1024,
                total: 12288 * 1024
            })
        );
        assert_eq!(gpu.watts, Some(53.76));
        let gpu = parse_nvidia("GPU, 400, -5, [N/A], 1e400, [Not Supported]").unwrap();
        assert_eq!(
            (gpu.busy, gpu.temp, gpu.vram, gpu.watts),
            (100, None, None, None)
        );
        assert!(parse_nvidia("too, few").is_none());
        assert!(parse_nvidia("a, 1, 2, 3, 4, 5, extra").is_none());
        assert!(parse_nvidia("").is_none());
        let long = "x".repeat(10_000);
        let gpu = parse_nvidia(&format!("{long}, 1, 1, 1, 1, 1")).unwrap();
        assert_eq!(gpu.name.map(|n| n.len()), Some(120));
    }
}
