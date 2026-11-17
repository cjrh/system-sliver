//! Only CPU, memory and network counters are refreshed; no process scanning.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use sysinfo::{Networks, System};

pub const RETENTION: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone)]
pub struct Sample {
    pub at: Instant,
    /// Expected sampling cadence when this observation was requested.
    pub interval: Duration,
    pub cpu: Option<f32>,
    pub cores: usize,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub download: Option<f32>,
    pub upload: Option<f32>,
}

#[cfg(test)]
impl Sample {
    /// Deterministic counters for tests that do not exercise OS sampling.
    pub fn fixture() -> Self {
        Self {
            at: Instant::now(),
            interval: Duration::from_secs(1),
            cpu: Some(25.0),
            cores: 4,
            memory_used: 50,
            memory_total: 100,
            swap_used: 0,
            swap_total: 0,
            download: Some(0.0),
            upload: Some(0.0),
        }
    }
}

#[derive(Default)]
pub struct History(pub VecDeque<Sample>);

impl History {
    pub fn push(&mut self, sample: Sample) {
        while self
            .0
            .front()
            .is_some_and(|old| sample.at.saturating_duration_since(old.at) > RETENTION)
        {
            self.0.pop_front();
        }
        self.0.push_back(sample);
    }
}

pub struct Sampler {
    system: System,
    networks: Networks,
    previous: Option<Instant>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            networks: Networks::new(),
            previous: None,
        }
    }

    pub fn sample(&mut self, interval: Duration) -> Sample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.networks.refresh(true);
        let at = Instant::now();
        let elapsed = self.previous.map(|previous| at.duration_since(previous));
        let mut received = 0_u64;
        let mut transmitted = 0_u64;
        // Exclude loopback. All other interfaces are aggregated, including VPNs
        // and bridges: this measures interface traffic, not Internet-only usage.
        for (name, interface) in &self.networks {
            if name == "lo" || name.starts_with("lo0") {
                continue;
            }
            received = received.saturating_add(interface.received());
            transmitted = transmitted.saturating_add(interface.transmitted());
        }
        let sample = Sample {
            at,
            interval,
            // CPU and network need two observations; don't show a false zero.
            cpu: elapsed.map(|_| self.system.global_cpu_usage()),
            cores: self.system.cpus().len(),
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            swap_used: self.system.used_swap(),
            swap_total: self.system.total_swap(),
            download: elapsed.map(|dt| rate(received, dt)),
            upload: elapsed.map(|dt| rate(transmitted, dt)),
        };
        self.previous = Some(at);
        sample
    }
}

fn rate(bytes: u64, elapsed: Duration) -> f32 {
    (bytes as f64 / elapsed.as_secs_f64().max(0.001)) as f32
}

pub fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32
    }
}

pub fn bytes(value: f32) -> String {
    let value = value.max(0.0);
    if value >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GiB", value / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024.0 * 1024.0 {
        format!("{:.1} MiB", value / (1024.0 * 1024.0))
    } else if value >= 1024.0 {
        format!("{:.1} KiB", value / 1024.0)
    } else {
        format!("{value:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_handle_absent_swap_and_bounds() {
        assert_eq!(percent(0, 0), 0.0);
        assert_eq!(percent(50, 200), 25.0);
        assert_eq!(percent(300, 200), 100.0);
    }

    #[test]
    fn rates_use_actual_elapsed_time() {
        assert_eq!(rate(4096, Duration::from_secs(2)), 2048.0);
        assert_eq!(bytes(2048.0), "2.0 KiB");
        assert_eq!(bytes(1048576.0), "1.0 MiB");
    }

    #[test]
    fn history_is_time_bounded() {
        assert_eq!(RETENTION, Duration::from_secs(3600));
        let sample = Sample::fixture();
        let mut history = History::default();
        history.push(sample.clone());
        let mut later = sample.clone();
        later.at = sample.at + RETENTION;
        history.push(later.clone());
        assert_eq!(history.0.len(), 2);
        later.at += Duration::from_secs(1);
        history.push(later);
        assert_eq!(history.0.len(), 2);
        assert!(history.0.front().unwrap().at > sample.at);
    }

    #[test]
    fn first_sample_has_no_misleading_rate() {
        let first = Sampler::new().sample(Duration::from_secs(1));
        assert!(first.cpu.is_none());
        assert!(first.download.is_none());
        assert!(first.upload.is_none());
        assert!(first.memory_used <= first.memory_total);
    }
}
