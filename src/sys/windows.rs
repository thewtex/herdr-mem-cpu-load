//! Windows backend: `GetSystemTimes`, `GlobalMemoryStatusEx`, `GetSystemInfo`,
//! and an emulated load average.
//!
//! A port of `windows/cpu.cc` and `windows/memory.cc` from `tmux-mem-cpu-load`,
//! but built on the kernel time counters instead of PDH. `GetSystemTimes` needs
//! no query handle, no counter path, and — unlike
//! `\\Processor(_Total)\\% Processor Time` — nothing that a localised Windows
//! install renames underneath it.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::SystemInformation::{
    GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use super::filetime::{cpu_times_from_system_times, filetime_to_u64};
use super::{cpu_count_fallback, CpuTimes, LoadAverages, MemoryStatus, SysError};
use crate::metrics::load_emulator::LoadEmulator;

/// The emulated load averages, shared by the whole process.
///
/// `load_averages` is a free function with nowhere to keep state, so the
/// daemon's [`LoadEmulator`] publishes its output here after every tick and
/// this is what the metric layer reads back.
static EMULATED_LOAD: Mutex<Option<LoadAverages>> = Mutex::new(None);

/// A lock on the shared load slot. Poisoning is irrelevant here: the value is a
/// plain triple of numbers that cannot be left half written, and a stale
/// reading beats taking the sampler down.
fn emulated_load() -> MutexGuard<'static, Option<LoadAverages>> {
    EMULATED_LOAD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Publish the daemon's emulated load averages.
pub(crate) fn publish_emulated_load(averages: LoadAverages) {
    *emulated_load() = Some(averages);
}

/// Seed the load averages from a single sample, unless something already
/// published a better answer.
///
/// One-line mode takes exactly one measurement and exits, so there is no
/// history to decay: the best estimate for all three windows is the busy core
/// count right now, which is what a fresh [`LoadEmulator`] returns from its
/// first update.
pub(crate) fn seed_emulated_load(busy_cores: f64) {
    let mut slot = emulated_load();
    if slot.is_none() {
        let mut emulator = LoadEmulator::new();
        *slot = Some(emulator.update(busy_cores, Duration::ZERO));
    }
}

/// Cumulative CPU time from `GetSystemTimes`.
pub(crate) fn cpu_times() -> Result<CpuTimes, SysError> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    // SAFETY: the three pointers each address one initialised, exclusively
    // borrowed `FILETIME`, which is all `GetSystemTimes` writes.
    let ok = unsafe { GetSystemTimes(&raw mut idle, &raw mut kernel, &raw mut user) };
    if ok == 0 {
        return Err(SysError::new("GetSystemTimes failed"));
    }

    Ok(cpu_times_from_system_times(
        filetime_to_u64(idle.dwHighDateTime, idle.dwLowDateTime),
        filetime_to_u64(kernel.dwHighDateTime, kernel.dwLowDateTime),
        filetime_to_u64(user.dwHighDateTime, user.dwLowDateTime),
    ))
}

/// Physical memory usage from `GlobalMemoryStatusEx`.
///
/// The original `windows/memory.cc` reported `ullAvailPhys` as *used* memory,
/// which is a bug: it printed the free half of the machine where every other
/// platform printed the busy half. That is not replicated — used memory here is
/// `ullTotalPhys - ullAvailPhys`.
pub(crate) fn memory_status() -> Result<MemoryStatus, SysError> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };

    // SAFETY: `status` is an initialised, exclusively borrowed `MEMORYSTATUSEX`
    // whose `dwLength` says how large it is, which is how the call knows how
    // much it may write.
    let ok = unsafe { GlobalMemoryStatusEx(&raw mut status) };
    if ok == 0 {
        return Err(SysError::new("GlobalMemoryStatusEx failed"));
    }

    Ok(MemoryStatus {
        used_bytes: status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
        total_bytes: status.ullTotalPhys,
    })
}

/// The emulated load averages.
///
/// Windows keeps no load average of its own, so this reports whatever the
/// daemon's [`LoadEmulator`] last published, or the single-sample seed that
/// one-line mode installs through [`seed_emulated_load`].
///
/// Reading a value the process itself computed cannot fail, but the backend
/// contract is the same on every platform, so the `Result` stays.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn load_averages() -> Result<LoadAverages, SysError> {
    Ok(emulated_load().unwrap_or_default())
}

/// Logical processors from `GetSystemInfo`.
pub(crate) fn cpu_count() -> u32 {
    let mut info = SYSTEM_INFO::default();
    // SAFETY: `info` is an initialised, exclusively borrowed `SYSTEM_INFO`,
    // which is the only thing `GetSystemInfo` writes.
    unsafe { GetSystemInfo(&raw mut info) };

    if info.dwNumberOfProcessors > 0 {
        info.dwNumberOfProcessors
    } else {
        cpu_count_fallback()
    }
}
