//! macOS backend: Mach `host_statistics`, `sysctlbyname`, and `getloadavg`.
//!
//! A port of `osx/cpu.cc` and `osx/memory.cc` from `tmux-mem-cpu-load`. The
//! Mach calls are all of the form "hand the kernel a struct and the number of
//! `integer_t`s it holds", so each one is wrapped in a small safe function that
//! states that invariant once.

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

use super::unix_common;
use super::{CpuTimes, MemoryStatus, SysError};

// `getloadavg` and `sysconf(_SC_NPROCESSORS_ONLN)` behave the same here as on
// Linux, so both backends share one implementation.
pub(crate) use unix_common::{cpu_count, load_averages};

/// Size of `host_cpu_load_info_data_t` in `integer_t` units, which is how Mach
/// counts an info buffer. Equal to `libc::HOST_CPU_LOAD_INFO_COUNT`.
const HOST_CPU_LOAD_INFO_COUNT: libc::mach_msg_type_number_t =
    (size_of::<libc::host_cpu_load_info_data_t>() / size_of::<i32>())
        as libc::mach_msg_type_number_t;

/// Size of `vm_statistics64_data_t` in `integer_t` units. Equal to
/// `libc::HOST_VM_INFO64_COUNT`.
const HOST_VM_INFO64_COUNT: libc::mach_msg_type_number_t =
    (size_of::<libc::vm_statistics64_data_t>() / size_of::<i32>()) as libc::mach_msg_type_number_t;

extern "C" {
    /// Not exposed by the `libc` crate. The symbol lives in libSystem, which
    /// every macOS binary links by default, so no `#[link]` is needed.
    fn host_page_size(
        host: libc::host_t,
        out_page_size: *mut libc::vm_size_t,
    ) -> libc::kern_return_t;
}

/// A send right to the host port.
///
/// `libc` marks `mach_host_self` deprecated in favour of the `mach2` crate.
/// This port stays dependency free on purpose, and the symbol itself is not
/// going anywhere, so the deprecation is allowed here rather than obeyed.
#[allow(deprecated)]
fn mach_host() -> libc::host_t {
    // SAFETY: `mach_host_self` takes no arguments and returns a plain port
    // name. The host-self name is a well-known special port that does not need
    // to be deallocated by the caller.
    unsafe { libc::mach_host_self() }
}

/// The cumulative per-state tick counters from `HOST_CPU_LOAD_INFO`.
fn host_cpu_load() -> Result<libc::host_cpu_load_info_data_t, SysError> {
    let mut info = MaybeUninit::<libc::host_cpu_load_info_data_t>::uninit();
    let mut count = HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: `count` is the size of `*info` measured in `integer_t`s, which is
    // exactly what `host_statistics` expects, and `info` points at storage of
    // that size. The kernel never writes more than `count` units.
    let status = unsafe {
        libc::host_statistics(
            mach_host(),
            libc::HOST_CPU_LOAD_INFO,
            info.as_mut_ptr().cast::<libc::integer_t>(),
            &raw mut count,
        )
    };
    if status != libc::KERN_SUCCESS {
        return Err(SysError::new(format!(
            "host_statistics(HOST_CPU_LOAD_INFO) failed with {status}"
        )));
    }
    // SAFETY: a `KERN_SUCCESS` return means the kernel filled the whole buffer.
    Ok(unsafe { info.assume_init() })
}

/// The virtual memory page counters from `HOST_VM_INFO64`.
fn host_vm_statistics() -> Result<libc::vm_statistics64_data_t, SysError> {
    let mut info = MaybeUninit::<libc::vm_statistics64_data_t>::uninit();
    let mut count = HOST_VM_INFO64_COUNT;
    // SAFETY: same contract as `host_cpu_load`; `count` is the size of `*info`
    // in `integer_t` units and the buffer is that large.
    let status = unsafe {
        libc::host_statistics64(
            mach_host(),
            libc::HOST_VM_INFO64,
            info.as_mut_ptr().cast::<libc::integer_t>(),
            &raw mut count,
        )
    };
    if status != libc::KERN_SUCCESS {
        return Err(SysError::new(format!(
            "host_statistics64(HOST_VM_INFO64) failed with {status}"
        )));
    }
    // SAFETY: a `KERN_SUCCESS` return means the kernel filled the whole buffer.
    Ok(unsafe { info.assume_init() })
}

/// Bytes per virtual memory page.
fn page_size() -> Result<u64, SysError> {
    let mut size: libc::vm_size_t = 0;
    // SAFETY: `host_page_size` writes exactly one `vm_size_t` through the
    // pointer, and `size` is one.
    let status = unsafe { host_page_size(mach_host(), &raw mut size) };
    if status != libc::KERN_SUCCESS {
        return Err(SysError::new(format!(
            "host_page_size failed with {status}"
        )));
    }
    Ok(size as u64)
}

/// Installed physical memory, from the `hw.memsize` sysctl.
fn physical_memory() -> Result<u64, SysError> {
    let mut total = 0_u64;
    let mut length = size_of::<u64>();
    // SAFETY: `length` is the size of `total`, so `sysctlbyname` cannot write
    // past it; the new-value pointer is null with a zero length because this is
    // a read.
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut total).cast::<c_void>(),
            &raw mut length,
            ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(SysError::new("sysctlbyname(hw.memsize) failed"));
    }
    Ok(total)
}

/// Cumulative CPU ticks.
///
/// Mach orders `cpu_ticks` as user, system, idle, nice, which is *not* the
/// order `/proc/stat` uses; the index constants keep the mapping honest.
pub(crate) fn cpu_times() -> Result<CpuTimes, SysError> {
    let load = host_cpu_load()?;
    Ok(CpuTimes {
        user: u64::from(load.cpu_ticks[libc::CPU_STATE_USER as usize]),
        nice: u64::from(load.cpu_ticks[libc::CPU_STATE_NICE as usize]),
        system: u64::from(load.cpu_ticks[libc::CPU_STATE_SYSTEM as usize]),
        idle: u64::from(load.cpu_ticks[libc::CPU_STATE_IDLE as usize]),
    })
}

/// Physical memory usage.
///
/// Used memory is `(active + wired) * page_size`, the formula the original
/// `osx/memory.cc` uses. macOS also keeps a compressed pool, and adding
/// `compressor_page_count` would report a larger — and arguably more honest —
/// number, but it is deliberately excluded so this reads the same as
/// `tmux-mem-cpu-load`.
pub(crate) fn memory_status() -> Result<MemoryStatus, SysError> {
    let total_bytes = physical_memory()?;
    let page_size = page_size()?;
    let stats = host_vm_statistics()?;
    let used_pages = u64::from(stats.active_count).saturating_add(u64::from(stats.wire_count));

    Ok(MemoryStatus {
        used_bytes: used_pages.saturating_mul(page_size),
        total_bytes,
    })
}
