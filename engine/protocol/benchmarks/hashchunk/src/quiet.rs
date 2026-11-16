// SPDX-License-Identifier: 0BSD
//! Host CPU accounting, excluding this process (all threads) and reaped children.
//! The hash pipeline spawns no processes and joins every thread before entering this gate.
//! Kernel workers and other processes remain counted, including asynchronous I/O aftermath.

use serde::Serialize;
use std::io;
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(120);
pub const INTERVAL: Duration = Duration::from_secs(1);
pub const REQUIRED_SAMPLES: usize = 3;

#[derive(Clone, Copy, Debug)]
struct Cpu {
    total: u64,
    idle: u64,
    own: u64,
}

fn parse_cpu(system: &str, process: &str) -> Option<Cpu> {
    let mut fields = system.lines().next()?.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    // guest/guest_nice are already included in user/nice: do not count them twice.
    let ticks: Vec<u64> = fields
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if ticks.len() != 8 {
        return None;
    }
    // comm (field 2) may itself contain spaces and parentheses. Start at field 3.
    let fields: Vec<&str> = process.rsplit_once(')')?.1.split_whitespace().collect();
    // /proc/self/stat utime + stime cover ALL threads; cutime + cstime cover waited-for children.
    let own = [11, 12, 13, 14].into_iter().try_fold(0u64, |sum, i| {
        sum.checked_add(fields.get(i)?.parse::<u64>().ok()?)
    })?;
    Some(Cpu {
        total: ticks.iter().sum(),
        idle: ticks[3] + ticks[4],
        own,
    })
}

fn read_cpu() -> io::Result<Cpu> {
    let system = std::fs::read_to_string("/proc/stat")?;
    let process = std::fs::read_to_string("/proc/self/stat")?;
    parse_cpu(&system, &process).ok_or_else(|| io::Error::other("invalid CPU accounting"))
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Busy {
    pub system_percent: f64,
    pub own_percent: f64,
    pub external_percent: f64,
}

fn difference(a: Cpu, b: Cpu) -> io::Result<Busy> {
    let invalid = || io::Error::other("CPU accounting unavailable or counters regressed");
    let total = b
        .total
        .checked_sub(a.total)
        .filter(|n| *n > 0)
        .ok_or_else(invalid)?;
    // Linux iowait may decrease; a regressing counter is an invalid sample, never quiet.
    let idle = b.idle.checked_sub(a.idle).ok_or_else(invalid)?;
    let busy = total.checked_sub(idle).ok_or_else(invalid)?;
    let own = b.own.checked_sub(a.own).ok_or_else(invalid)?;
    if own > total {
        return Err(invalid());
    }
    // /proc reads are not atomic; clamp a tiny accounting skew at zero external CPU.
    Ok(Busy {
        system_percent: 100.0 * busy as f64 / total as f64,
        own_percent: 100.0 * own as f64 / total as f64,
        external_percent: 100.0 * busy.saturating_sub(own) as f64 / total as f64,
    })
}

#[derive(Debug, Serialize)]
pub struct Sample {
    pub elapsed_seconds: f64,
    /// None means the CPU accounting failed; it resets the quiet streak.
    pub busy: Option<Busy>,
}

#[derive(Debug, Serialize)]
pub struct Wait {
    pub wait_seconds: f64,
    pub ready: bool,
    /// All samples (at most 120 at the normal cadence), including the final three.
    pub samples: Vec<Sample>,
}

// The clock/sampler boundary lets tests exercise the full deadline without real sleeps.
fn wait_with(limit: f64, mut next: impl FnMut(Duration) -> (Duration, io::Result<Busy>)) -> Wait {
    let mut elapsed = Duration::ZERO;
    let mut streak = 0;
    let mut samples = Vec::new();
    while elapsed < TIMEOUT {
        let interval = INTERVAL.min(TIMEOUT - elapsed);
        let (time, sample) = next(interval);
        elapsed = time;
        let busy = sample.ok();
        // A shortened final interval exhausts the deadline but cannot stand in for a full 1 s quiet sample.
        if interval == INTERVAL
            && busy.is_some_and(|b| {
                b.external_percent.is_finite()
                    && b.external_percent >= 0.0
                    && b.external_percent <= limit
            })
        {
            streak += 1;
        } else {
            streak = 0;
        }
        samples.push(Sample {
            elapsed_seconds: elapsed.as_secs_f64(),
            busy,
        });
        if streak >= REQUIRED_SAMPLES && elapsed <= TIMEOUT {
            return Wait {
                wait_seconds: elapsed.as_secs_f64(),
                ready: true,
                samples,
            };
        }
    }
    Wait {
        wait_seconds: elapsed.as_secs_f64(),
        ready: false,
        samples,
    }
}

pub fn wait(limit: f64) -> Wait {
    let start = Instant::now();
    let mut previous = read_cpu();
    wait_with(limit, |duration| {
        // Include procfs reads in the 120 s budget as well as the sample intervals.
        std::thread::sleep(duration.min(TIMEOUT.saturating_sub(start.elapsed())));
        let current = read_cpu();
        let sample = match (&previous, &current) {
            (Ok(a), Ok(b)) => difference(*a, *b),
            _ => Err(io::Error::other("CPU accounting unavailable")),
        };
        previous = current;
        (start.elapsed(), sample)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripted(values: &[Option<f64>]) -> Wait {
        let mut time = Duration::ZERO;
        let mut i = 0;
        wait_with(10.0, |duration| {
            time += duration;
            let value = values[i.min(values.len() - 1)];
            i += 1;
            (
                time,
                value
                    .map(|v| Busy {
                        system_percent: v,
                        own_percent: 0.0,
                        external_percent: v,
                    })
                    .ok_or_else(|| io::Error::other("probe failed")),
            )
        })
    }

    #[test]
    fn waits_for_three_consecutive_samples_and_resets_on_busy_or_probe_failure() {
        let w = scripted(&[
            Some(89.2),
            Some(10.0),
            Some(0.0),
            Some(12.0),
            Some(0.0),
            None,
            Some(10.0),
            Some(3.0),
            Some(1.0),
        ]);
        assert!(w.ready);
        assert_eq!(w.wait_seconds, 9.0);
        assert_eq!(w.samples.len(), 9);
        assert!(w.samples[5].busy.is_none());
    }

    #[test]
    fn persistent_load_or_unavailable_accounting_times_out_at_120_seconds() {
        for values in [
            &[Some(80.9)][..],
            &[None][..],
            &[Some(f64::NAN)][..],
            &[Some(-1.0)][..],
        ] {
            let w = scripted(values);
            assert!(!w.ready);
            assert_eq!(w.wait_seconds, 120.0);
            assert_eq!(w.samples.len(), 120);
        }
        let mut values = vec![Some(89.2); 117];
        values.extend([Some(10.0); 3]);
        assert!(
            scripted(&values).ready,
            "third quiet sample at deadline must pass"
        );
    }

    #[test]
    fn shortened_final_interval_or_late_sample_cannot_complete_the_quiet_streak() {
        for times in [[118.5, 119.5, 120.0], [1.0, 2.0, 121.0]] {
            let mut i = 0;
            let w = wait_with(10.0, |_| {
                let time = Duration::from_secs_f64(times[i]);
                i += 1;
                (
                    time,
                    Ok(Busy {
                        system_percent: 0.0,
                        own_percent: 0.0,
                        external_percent: 0.0,
                    }),
                )
            });
            assert!(!w.ready);
        }
    }

    #[test]
    fn subtracts_own_threads_and_waited_children_without_renormalizing_host_capacity() {
        let mut fields = vec!["0"; 15];
        fields[0] = "R";
        fields[11..15].copy_from_slice(&["100", "20", "30", "4"]);
        let process = format!("1 (name with ) parentheses) {}", fields.join(" "));
        let cpu = parse_cpu("cpu 100 0 50 800 10 20 10 10 100 0\n", &process).unwrap();
        assert_eq!(cpu.total, 1000);
        assert_eq!(cpu.idle, 810);
        assert_eq!(cpu.own, 154);
        let b = difference(
            cpu,
            Cpu {
                total: 1100,
                idle: 830,
                own: 224,
            },
        )
        .unwrap();
        assert_eq!(b.system_percent, 80.0);
        assert_eq!(b.own_percent, 70.0);
        assert_eq!(b.external_percent, 10.0);
        assert!(difference(
            cpu,
            Cpu {
                total: 1100,
                idle: 809,
                own: 154
            }
        )
        .is_err());
        assert!(parse_cpu("cpu broken", &process).is_none());
    }
}
