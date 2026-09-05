//! Fallback backend for platforms without a native implementation.

use super::{cpu_count_fallback, CpuTimes, LoadAverages, MemoryStatus, SysError};

fn unsupported() -> SysError {
    SysError::new("unsupported platform")
}

pub(crate) fn cpu_times() -> Result<CpuTimes, SysError> {
    Err(unsupported())
}

pub(crate) fn memory_status() -> Result<MemoryStatus, SysError> {
    Err(unsupported())
}

pub(crate) fn load_averages() -> Result<LoadAverages, SysError> {
    Err(unsupported())
}

pub(crate) fn cpu_count() -> u32 {
    cpu_count_fallback()
}
