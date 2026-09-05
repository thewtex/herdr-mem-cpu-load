# Phase 03: macOS and Windows Backends

This phase completes the platform matrix declared in the manifest. macOS gets a backend built on Mach `host_statistics`, `sysctl`, and `getloadavg`, porting `/home/matt/src/tmux-mem-cpu-load/osx/`. Windows gets `GetSystemTimes` and `GlobalMemoryStatusEx` through `windows-sys`, plus an emulated 1/5/15-minute load average (Windows has no native load average; the original Windows port simply printed nothing). The BSDs receive a compile-time stub. Because this machine is Linux, correctness is verified by type-checking every target with `cargo check --target ...` and by unit tests on the pure math, with real execution deferred to the CI matrix in Phase 05.

## Tasks

- [ ] Add the platform dependencies and target-specific configuration to `Cargo.toml`:
  - `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Threading", "Win32_System_SystemInformation"] }` (`GetSystemTimes` lives in `Win32_System_Threading`; `GlobalMemoryStatusEx`, `MEMORYSTATUSEX`, `GetSystemInfo`, and `SYSTEM_INFO` live in `Win32_System_SystemInformation`).
  - Keep `libc` for all Unix targets (Linux, macOS, BSDs).
  - Create `src/sys/macos.rs` and `src/sys/windows.rs` and wire the `cfg` arms in `src/sys/mod.rs` that Phase 01 left guarded: `#[cfg(target_os = "macos")] mod macos;`, `#[cfg(windows)] mod windows;`, with the `unsupported` module covering everything else.

- [ ] Implement the macOS backend in `src/sys/macos.rs`, porting `osx/cpu.cc` and `osx/memory.cc`:
  - First check which of these the `libc` crate already exposes for Apple targets (search the vendored `libc` sources under `~/.cargo/registry/src/*/libc-*/src/unix/bsd/apple/`): `mach_host_self`, `host_statistics`, `host_statistics64`, `host_page_size`, `sysctlbyname`, `getloadavg`, `sysconf`, `vm_statistics64`, `host_cpu_load_info`. Use the `libc` items where they exist; otherwise declare the minimal `extern "C"` bindings and `#[repr(C)]` structs locally in the module (the symbols come from the default-linked System framework, so no `#[link]` attribute is required). Required constants: `HOST_CPU_LOAD_INFO = 3`, `CPU_STATE_USER = 0`, `CPU_STATE_SYSTEM = 1`, `CPU_STATE_IDLE = 2`, `CPU_STATE_NICE = 3`, `HOST_VM_INFO64 = 4`, `KERN_SUCCESS = 0`, counts computed as `size_of::<Struct>() / size_of::<i32>()`.
  - `cpu_times()`: call `host_statistics(mach_host_self(), HOST_CPU_LOAD_INFO, ptr, &mut count)` and map the `cpu_ticks[4]` array to `CpuTimes` using the index constants above (note the ordering differs from Linux: user, system, idle, nice). Non-`KERN_SUCCESS` returns `SysError`.
  - `memory_status()`: total from `sysctlbyname("hw.memsize")` into a `u64`; used from `host_statistics64(HOST_VM_INFO64)` as `(active_count + wire_count) * page_size` where `page_size` comes from `host_page_size`, matching the original formula. Add a doc comment noting that `compressor_page_count` is intentionally excluded to stay faithful to tmux-mem-cpu-load.
  - `load_averages()` and `cpu_count()` share the Unix implementation with Linux; move those two functions into `src/sys/unix_common.rs` (`#[cfg(unix)]`) and have both `linux.rs` and `macos.rs` re-export them instead of duplicating code.
  - Wrap every `unsafe` block in a small safe function with a `// SAFETY:` comment explaining the invariant (buffer sizes and count arguments).

- [ ] Implement the Windows backend in `src/sys/windows.rs`, porting `windows/cpu.cc` and `windows/memory.cc` but using the kernel time counters instead of PDH (no query handles, no counter names to localize):
  - `cpu_times()`: call `GetSystemTimes(&mut idle, &mut kernel, &mut user)`; convert each `FILETIME` to `u64` via `(dwHighDateTime << 32) | dwLowDateTime` in a pure helper `filetime_to_u64`. Because `kernel` includes idle time on Windows, map to `CpuTimes { user, nice: 0, system: kernel - idle, idle }` so the shared busy/total formula produces the right percentage.
  - `memory_status()`: `GlobalMemoryStatusEx` with `dwLength` set to `size_of::<MEMORYSTATUSEX>()`; `total_bytes = ullTotalPhys`, `used_bytes = ullTotalPhys - ullAvailPhys` (the original Windows port stored available memory as used, which was a bug; do not replicate it, and say so in a doc comment).
  - `cpu_count()`: `GetSystemInfo` and read `dwNumberOfProcessors`, with the same fallback chain as other platforms.
  - `load_averages()`: return the current values from a process-wide `LoadEmulator` (below). In one-line mode there is only one sample, so `load_averages()` seeds the emulator from the current CPU percentage and returns that as all three values; document this limitation in the README platform notes.

- [ ] Implement the platform-independent load average emulation in `src/metrics/load_emulator.rs` (compiled on every platform so it is unit tested on Linux, but only used by the Windows backend and, later, as an optional `--emulate-load` flag):
  - `pub struct LoadEmulator { one: f64, five: f64, fifteen: f64, seeded: bool }` with `pub fn update(&mut self, busy_cores: f64, dt: Duration) -> LoadAverages`. Use the Linux kernel formula with continuous decay: for each window `T` in (60, 300, 900) seconds, `factor = (-dt.as_secs_f64() / T).exp()`, `value = value * factor + busy_cores * (1.0 - factor)`. On the first call, set all three to `busy_cores`.
  - `busy_cores` is `cpu_percent / 100 * cpu_count` computed by the daemon or one-line sampler. Expose `pub fn current(&self) -> LoadAverages`.
  - Wire the daemon so that on Windows the `LoadEmulator` is updated every tick before `metrics::collect` reads `load_averages()`; keep this behind `cfg(windows)` in `daemon.rs` via a tiny `LoadSource` enum (`Native` | `Emulated(LoadEmulator)`) so the Linux/macOS code path is unchanged.

- [ ] Write unit tests for the new pure logic:
  - `load_emulator.rs`: after a single update the three values equal `busy_cores`; a constant `busy_cores` of 2.0 stays at 2.0 across many updates; after switching from 0.0 to 4.0 for 60 seconds the one-minute value is approximately `4.0 * (1 - e^-1)` (≈ 2.53, tolerance 0.05) while the fifteen-minute value is lower; values never go negative or exceed the input maximum.
  - `sys/windows.rs` (with `#[cfg(windows)]` on the OS calls but the helper tested everywhere by moving `filetime_to_u64` into `src/sys/filetime.rs` compiled unconditionally): `filetime_to_u64(high 1, low 5)` is `(1 << 32) + 5`; a `CpuTimes` built from idle 100 / kernel 300 / user 200 yields busy 400 of total 500 (80 percent).
  - `sys/mod.rs`: `CpuTimes::delta` saturates instead of panicking when counters go backwards.

- [ ] Type-check every declared platform from this Linux machine using rustup targets (this needs no cross linker because `cargo check` does not link):
  - `rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-pc-windows-gnu`.
  - `cargo check --target aarch64-apple-darwin`, `cargo check --target x86_64-apple-darwin`, `cargo check --target x86_64-pc-windows-msvc`, `cargo check --target x86_64-pc-windows-gnu`, and `cargo clippy --target <each> -- -D warnings`. Fix every error and warning (wrong `windows-sys` feature names, missing `libc` items, unused imports behind `cfg`) until all four are clean.
  - Also `cargo check --target x86_64-unknown-freebsd` after `rustup target add x86_64-unknown-freebsd` to confirm the `unsupported` stub compiles (if the rust-std component is unavailable for that target, note it in the commit message and move on).
  - `cargo test` and `cargo clippy --all-targets -- -D warnings` on the host must still pass.

- [ ] Document the platform matrix in `README.md` under a "Platforms" section: Linux (`/proc/stat`, `/proc/meminfo`, `getloadavg`), macOS (`host_statistics`, `hw.memsize`, `getloadavg`), Windows (`GetSystemTimes`, `GlobalMemoryStatusEx`, emulated load average with the formula), FreeBSD/OpenBSD/NetBSD (not yet supported; contributions welcome, pointing at the corresponding `.cc` files in tmux-mem-cpu-load as the porting reference). Note in `herdr-plugin.toml` comments that the Windows build needs the MSVC or GNU toolchain that `cargo` is already configured with.

- [ ] Commit with the message `feat: add macOS and Windows backends with emulated load average`.
