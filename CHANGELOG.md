# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The version here, in `Cargo.toml`, and in `herdr-plugin.toml` are bumped
together by `scripts/release.sh`; CI fails if they drift.

## [Unreleased]

### Added

- One-line status output ported from `tmux-mem-cpu-load`: memory, a CPU bar
  graph, the CPU percentage, and the one, five, and fifteen minute load
  averages, byte for byte what the uncoloured original prints.
- The original's command line, flag for flag: `-i`, `-g`, `-m`, `-t`, `-a`,
  `-v`, `-c`, `-p`, `-q`, `-l`, and `-r`.
- Three CPU graph styles: the original ASCII `classic` bar, a framed
  eighth-resolution `blocks` bar, and the single-character `vertical` one.
- tmux colour markup and powerline separators from the original's lookup
  tables, plus `--ansi` for the same colours as 256-colour SGR escapes.
- `--daemon`: a resident sampler that reports Space sidebar tokens to every
  open workspace over the `herdr` CLI. Fourteen tokens per report —
  `$cpu_status`, `$mem_status`, `$load_status`, `$sys_status`, `$cpu_history`,
  and an `_ok`/`_warn`/`_hot` level token for each of the three metrics — with
  the levels held by hysteresis so a metric sitting on a threshold does not
  repaint its row every tick.
- A singleton lock (`flock` on Unix, `LockFileEx` on Windows) so a herdr live
  handoff, which re-runs `[[startup]]`, cannot double up the daemon.
- `workspaces = "focused"` (`--workspaces focused`): report the sidebar rows to
  the active workspace alone instead of every open one, so one machine's
  numbers are not repeated under every Space. The rows follow the focus — the
  workspace being left has its tokens cleared on the same tick, not at the end
  of the ttl — and a list with nothing focused holds them where they are.
- Live configuration reload: the daemon stats its `config.toml` once a tick and
  re-runs the whole merge when the file changes, so an edit lands within one
  interval instead of needing a restart. The command line still wins over the
  reloaded file, the CPU history is resized rather than discarded, and a file
  that does not parse leaves the daemon on the settings it already has.
- `herdr-plugin.toml`: build command, startup hooks per platform, a `status`
  popup pane, and the `show-status` and `write-config` actions.
- macOS backend over Mach `host_statistics`, `host_statistics64`, and
  `hw.memsize`; Windows backend over `GetSystemTimes`, `GlobalMemoryStatusEx`,
  and `GetSystemInfo`. Windows has no kernel load average, so the daemon
  emulates one with the same exponentially weighted moving average the Linux
  kernel uses. FreeBSD, OpenBSD, and NetBSD compile against a stub that fails
  cleanly.
- A `config.toml` layer in the plugin's config directory, merged under the
  command line, with `--print-config` to show the result and
  `--write-default-config` to drop a commented template.
- Configurable `[thresholds]` for the level tokens, and `mem_graph_lines` and
  `load_graph_lines` so the memory and load bars can be narrower than the CPU
  bar. Both fall back to `graph_lines`, so the three still move together
  unless they are set apart.
- `--watch`: re-sample every interval and rewrite the line in place, which is
  what the plugin's popup pane runs.
- Cross-platform CI (format, lint, test, release build, and a smoke run of the
  real binary on Linux, macOS, and Windows), a tag-driven release workflow, and
  end-to-end integration tests over the built binary.

### Fixed

- Windows used memory. The original `windows/memory.cc` reported `ullAvailPhys`
  as *used* memory, printing the free half of the machine where every other
  platform printed the busy half. Used memory here is
  `ullTotalPhys - ullAvailPhys`.
