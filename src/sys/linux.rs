//! Linux backend: `/proc/stat`, `/proc/meminfo`, and `getloadavg`.
//!
//! The parsing is split into pure functions taking `&str` so it can be unit
//! tested without touching the running system.

use std::fs;

use super::unix_common;
use super::{CpuTimes, MemoryStatus, SysError};

// `getloadavg` and `sysconf(_SC_NPROCESSORS_ONLN)` behave the same here as on
// macOS, so both backends share one implementation.
pub(crate) use unix_common::{cpu_count, load_averages};

const PROC_STAT: &str = "/proc/stat";
const PROC_MEMINFO: &str = "/proc/meminfo";

/// Parse the aggregate `cpu` line of `/proc/stat`.
///
/// Only the first four fields (user, nice, system, idle) are used, matching the
/// formula of the original C++ implementation; `iowait` and friends are ignored.
///
/// # Errors
///
/// Returns a [`SysError`] when the first line is missing, is not the aggregate
/// `cpu` line, or has fewer than four numeric fields.
pub(crate) fn parse_proc_stat(contents: &str) -> Result<CpuTimes, SysError> {
    let line = contents
        .lines()
        .next()
        .ok_or_else(|| SysError::new(format!("{PROC_STAT} is empty")))?;

    let mut fields = line.split_whitespace();
    if fields.next() != Some("cpu") {
        return Err(SysError::new(format!(
            "{PROC_STAT} does not start with the aggregate `cpu` line"
        )));
    }

    let mut values = [0_u64; 4];
    for slot in &mut values {
        let field = fields.next().ok_or_else(|| {
            SysError::new(format!("{PROC_STAT} `cpu` line has fewer than four fields"))
        })?;
        *slot = field.parse().map_err(|_| {
            SysError::new(format!("{PROC_STAT} `cpu` field `{field}` is not a number"))
        })?;
    }

    Ok(CpuTimes {
        user: values[0],
        nice: values[1],
        system: values[2],
        idle: values[3],
    })
}

/// Parse `/proc/meminfo` into a [`MemoryStatus`].
///
/// Linux uses spare RAM for disk caching, so `MemTotal - MemFree` overstates the
/// real usage. This reproduces the htop-style formula of the original:
/// `used = MemTotal - MemFree - Buffers - Cached - SReclaimable + Shmem`.
/// `Shmem` and `SReclaimable` are optional and count as zero when absent.
///
/// # Errors
///
/// Returns a [`SysError`] when `MemTotal` or `MemFree` is missing.
pub(crate) fn parse_meminfo(contents: &str) -> Result<MemoryStatus, SysError> {
    let mut total_kb: Option<u64> = None;
    let mut free_kb: Option<u64> = None;
    let mut buffers_kb = 0_u64;
    let mut cached_kb = 0_u64;
    let mut sreclaimable_kb = 0_u64;
    let mut shmem_kb = 0_u64;

    for line in contents.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(token) = rest.split_whitespace().next() else {
            continue;
        };
        let Ok(value) = token.parse::<u64>() else {
            continue;
        };

        match key.trim() {
            "MemTotal" => total_kb = Some(value),
            "MemFree" => free_kb = Some(value),
            "Buffers" => buffers_kb = value,
            "Cached" => cached_kb = value,
            "SReclaimable" => sreclaimable_kb = value,
            "Shmem" => shmem_kb = value,
            _ => {}
        }
    }

    let total_kb =
        total_kb.ok_or_else(|| SysError::new(format!("{PROC_MEMINFO} is missing MemTotal")))?;
    let free_kb =
        free_kb.ok_or_else(|| SysError::new(format!("{PROC_MEMINFO} is missing MemFree")))?;

    let used_kb = total_kb
        .saturating_sub(free_kb)
        .saturating_sub(buffers_kb)
        .saturating_sub(cached_kb)
        .saturating_sub(sreclaimable_kb)
        .saturating_add(shmem_kb);

    Ok(MemoryStatus {
        used_bytes: used_kb.saturating_mul(1024),
        total_bytes: total_kb.saturating_mul(1024),
    })
}

pub(crate) fn cpu_times() -> Result<CpuTimes, SysError> {
    parse_proc_stat(&fs::read_to_string(PROC_STAT)?)
}

pub(crate) fn memory_status() -> Result<MemoryStatus, SysError> {
    parse_meminfo(&fs::read_to_string(PROC_MEMINFO)?)
}

#[cfg(test)]
mod tests {
    use super::{parse_meminfo, parse_proc_stat};

    const KB: u64 = 1024;

    #[test]
    fn parses_the_aggregate_cpu_line() {
        let times = parse_proc_stat("cpu  4705 150 1120 16250 520 0 30 0 0 0\ncpu0 1 2 3 4\n")
            .expect("aggregate line parses");
        assert_eq!(times.user, 4705);
        assert_eq!(times.nice, 150);
        assert_eq!(times.system, 1120);
        assert_eq!(times.idle, 16_250);
        assert_eq!(times.busy(), 4705 + 150 + 1120);
        assert_eq!(times.total(), 4705 + 150 + 1120 + 16_250);
    }

    #[test]
    fn rejects_malformed_proc_stat() {
        assert!(parse_proc_stat("").is_err(), "empty input");
        assert!(
            parse_proc_stat("cpu0 4705 150 1120 16250\n").is_err(),
            "per-core line instead of the aggregate line"
        );
        assert!(
            parse_proc_stat("cpu  4705 150\n").is_err(),
            "fewer than four fields"
        );
        assert!(
            parse_proc_stat("cpu  4705 150 oops 16250\n").is_err(),
            "non-numeric field"
        );
    }

    #[test]
    fn parses_meminfo_with_the_htop_formula() {
        let fixture = "\
MemTotal:       16003 kB
MemFree:         4000 kB
MemAvailable:    9000 kB
Buffers:          500 kB
Cached:          3000 kB
SwapCached:        42 kB
Shmem:            200 kB
SReclaimable:     300 kB
";
        let status = parse_meminfo(fixture).expect("fixture parses");
        // 16003 - 4000 - 500 - 3000 - 300 + 200
        assert_eq!(status.used_bytes, 8403 * KB);
        assert_eq!(status.total_bytes, 16_003 * KB);
        assert_eq!(status.free_bytes(), (16_003 - 8403) * KB);
    }

    #[test]
    fn treats_missing_optional_keys_as_zero() {
        let fixture = "\
MemTotal:       16003 kB
MemFree:         4000 kB
Buffers:          500 kB
Cached:          3000 kB
";
        let status = parse_meminfo(fixture).expect("fixture without Shmem/SReclaimable parses");
        assert_eq!(status.used_bytes, 8503 * KB);
        assert_eq!(status.total_bytes, 16_003 * KB);
    }

    #[test]
    fn requires_memtotal_and_memfree() {
        assert!(parse_meminfo("MemFree: 4000 kB\n").is_err(), "no MemTotal");
        assert!(parse_meminfo("MemTotal: 16003 kB\n").is_err(), "no MemFree");
    }
}
