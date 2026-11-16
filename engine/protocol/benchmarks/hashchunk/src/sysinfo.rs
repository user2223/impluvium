// SPDX-License-Identifier: 0BSD
//! Machine description that goes into every result file.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::Path};

#[derive(Serialize, Clone)]
pub struct Machine {
    pub tool_version: &'static str,
    /// Explicitly supplied source revision; unknown when not recorded.
    pub source_revision: Option<String>,
    pub rustc: Option<String>,
    pub dependencies: BTreeMap<String, String>,
    pub cpu_model: String,
    pub logical_cpus: usize,
    pub available_parallelism: usize,
    pub cpu_mhz_first_core: Option<f64>,
    pub cpu_max_mhz: Option<f64>,
    pub flags: Flags,
    pub mem_total_bytes: u64,
    pub cgroup_cpu_max: Option<String>,
    pub cgroup_memory_max: Option<String>,
    pub kernel: String,
    pub arch: &'static str,
    pub os: &'static str,
}

#[derive(Serialize, Clone)]
pub struct Flags {
    pub sha_ni: bool,
    pub avx2: bool,
    pub avx512f: bool,
    pub sse41: bool,
}

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn collect(metadata: &RunMetadata) -> Machine {
    let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
    let model = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let mhz = cpuinfo
        .lines()
        .find(|l| l.starts_with("cpu MHz"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|s| s.trim().parse().ok());
    let logical = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();
    let mem_kb: u64 = read("/proc/meminfo")
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0);
    let max_mhz = read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|k| k / 1000.0);
    #[cfg(target_arch = "x86_64")]
    let flags = Flags {
        sha_ni: is_x86_feature_detected!("sha"),
        avx2: is_x86_feature_detected!("avx2"),
        avx512f: is_x86_feature_detected!("avx512f"),
        sse41: is_x86_feature_detected!("sse4.1"),
    };
    #[cfg(not(target_arch = "x86_64"))]
    let flags = Flags {
        sha_ni: false,
        avx2: false,
        avx512f: false,
        sse41: false,
    };
    Machine {
        tool_version: env!("CARGO_PKG_VERSION"),
        source_revision: metadata.source_revision.clone(),
        rustc: metadata.rustc.clone(),
        dependencies: metadata.dependencies.clone(),
        cpu_model: model,
        logical_cpus: logical,
        available_parallelism: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        cpu_mhz_first_core: mhz,
        cpu_max_mhz: max_mhz,
        flags,
        mem_total_bytes: mem_kb * 1024,
        cgroup_cpu_max: read("/sys/fs/cgroup/cpu.max"),
        cgroup_memory_max: read("/sys/fs/cgroup/memory.max"),
        kernel: read("/proc/sys/kernel/osrelease").unwrap_or_default(),
        arch: std::env::consts::ARCH,
        os: std::env::consts::OS,
    }
}

/// "1min 5min 15min running/total" from /proc/loadavg, for the quiet-window evidence in hash results.
pub fn loadavg() -> String {
    read("/proc/loadavg").unwrap_or_default()
}

/// Supplied by the caller, never inferred from a build script or shell variable.
#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunMetadata {
    pub source_revision: Option<String>,
    pub rustc: Option<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}
const DEPENDENCIES: [(&str, &str); 12] = [
    ("impluvium-protocol", "0.1.0"),
    ("buffa", "0.9.2"),
    ("buffa-types", "0.9.2"),
    ("fastcdc", "5.0.0"),
    ("sha2", "0.11.1"),
    ("blake3", "1.8.7"),
    ("serde", "1.0.229"),
    ("serde_json", "1.0.154"),
    ("clap", "4.6.7"),
    ("libc", "0.2.191"),
    ("rayon", "1.12.0"),
    ("crossbeam-channel", "0.5.17"),
];
impl RunMetadata {
    pub fn load(path: Option<&Path>) -> io::Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let value: Self =
            serde_json::from_slice(&std::fs::read(path)?).map_err(crate::util::io_err)?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> io::Result<()> {
        if self.source_revision.as_ref().is_some_and(|s| {
            s.len() != 40
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) || self
            .rustc
            .as_ref()
            .is_some_and(|s| !s.starts_with("rustc ") || s.trim().len() <= 6)
            || DEPENDENCIES.iter().any(|(name, expected)| {
                self.dependencies
                    .get(*name)
                    .is_some_and(|v| v.as_str() != *expected)
            })
        {
            return Err(io::Error::other(
                "invalid run metadata or dependency version differs from the tool pins",
            ));
        }
        Ok(())
    }
    pub fn complete(&self) -> bool {
        self.validate().is_ok()
            && self.source_revision.is_some()
            && self.rustc.is_some()
            && DEPENDENCIES.iter().all(|(name, _)| {
                (!cfg!(target_os = "linux") && *name == "libc")
                    || self.dependencies.contains_key(*name)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_metadata_is_unknown_and_wrong_metadata_is_rejected() {
        let absent = RunMetadata::load(None).unwrap();
        assert!(!absent.complete());
        let m = collect(&absent);
        assert!(m.source_revision.is_none() && m.rustc.is_none() && m.dependencies.is_empty());
        let mut full = RunMetadata {
            source_revision: Some("a".repeat(40)),
            rustc: Some("rustc 1.92.0 (example)".into()),
            dependencies: DEPENDENCIES
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        assert!(full.complete());
        full.dependencies.insert("fastcdc".into(), "0.0.0".into());
        assert!(full.validate().is_err());
        full.dependencies.remove("fastcdc");
        assert!(full.validate().is_ok() && !full.complete());
        full.source_revision = Some("invalid".into());
        assert!(full.validate().is_err());
        assert!(serde_json::from_str::<RunMetadata>(r#"{"rustc":42}"#).is_err());
        assert!(serde_json::from_str::<RunMetadata>(r#"{"unknown":"field"}"#).is_err());
    }
}
