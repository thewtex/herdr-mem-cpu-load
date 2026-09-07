# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The version here, in `Cargo.toml`, and in `herdr-plugin.toml` are bumped
together by `scripts/release.sh`; CI fails if they drift.

## [Unreleased]

## [0.1.1] - 2026-09-07

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
- `--daemon`: a resident sampler that reports Space sidebar tokens to the
  focused workspace over the `herdr` CLI. Fourteen tokens per report —
  `$cpu_status`, `$mem_status`, `$load_status`, `$sys_status`, `$cpu_history`,
  and an `_ok`/`_warn`/`_hot` level token for each of the three metrics — with
  the levels held by hysteresis so a metric sitting on a threshold does not
  repaint its row every tick.
- A singleton lock (`flock` on Unix, `LockFileEx` on Windows) so a herdr live
  handoff, which re-runs `[[startup]]`, cannot double up the daemon.
- `workspaces`: which workspaces the sidebar rows are reported to. The default
  is `"focused"`, the active workspace alone, so one machine's numbers are not
  repeated under every Space; the rows follow the focus, and the workspace
  being left has its tokens cleared on the same tick rather than at the end of
  the ttl. A list with nothing focused holds them where they are.
  `workspaces = "all"` (`--workspaces all`) restores a copy under every open
  workspace.
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
  bar. Both fall back to `graph_lines` when it is set, so the three still move
  together unless they are set apart; left alone the load bar defaults to four
  cells, since load is the coarsest of the three readings. `history`, the
  `$cpu_history` window, follows `graph_lines` the same way and defaults to
  twenty samples.
- `--write-sidebar-rows`, run by the manifest's second `[[build]]` step, so
  installing the plugin lays out `[ui.sidebar.spaces]` in herdr's own
  `config.toml` and the rows appear without a block being copied out of the
  README. It fills an empty table only: a sidebar someone has already arranged
  is left alone, as is a file that cannot be read or does not parse, and every
  outcome exits 0 so the install is never failed by it. The rest of the file —
  every other key, comment, and blank line — comes through untouched, and the
  new text is renamed over the old in one step, following a symlinked
  `config.toml` to the file it names rather than replacing the link.
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
