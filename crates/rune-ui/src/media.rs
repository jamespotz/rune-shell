//! Media formatting and visualizer sampling.
use std::time::Duration;

pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Averages `bars` (0..=100) down to `n` values in 0..=1.
pub fn downsample(bars: &[u8], n: usize) -> Vec<f32> {
    if bars.is_empty() {
        return Vec::new();
    }
    let chunk = bars.len().div_ceil(n);
    bars.chunks(chunk)
        .map(|c| c.iter().map(|b| f32::from(*b)).sum::<f32>() / (c.len() as f32 * 100.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(clock(Duration::from_secs(65)), "1:05");
        assert_eq!(clock(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn downsamples_bars() {
        assert_eq!(downsample(&[100, 0, 50, 50], 2), [0.5, 0.5]);
        assert!(downsample(&[], 5).is_empty());
        assert_eq!(downsample(&[100; 24], 5).len(), 5);
    }
}
