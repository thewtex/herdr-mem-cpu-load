# herdr-mem-cpu-load

[![CI](https://github.com/thewtex/herdr-mem-cpu-load/actions/workflows/ci.yml/badge.svg)](https://github.com/thewtex/herdr-mem-cpu-load/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

CPU, memory, and load average monitor for [herdr](https://github.com/thewtex/herdr),
re-imagined in Rust from [tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load).

One binary, two jobs. Run it plain and it prints the one line the original
prints, tmux colours and all, so it is a drop-in replacement in a
`status-left`. Run it with `--daemon` and it becomes the herdr plugin of the
same name, feeding live CPU, memory, and load rows to every workspace in the
Space sidebar.

## Overview

The daemon samples the machine every interval and reports a handful of Space
sidebar tokens to each open workspace — or to the active one alone, with
`workspaces = "focused"`. Put the ones you want in `[ui.sidebar.spaces]` and
the sidebar grows system rows that change colour as the machine heats up:

```
┌─ Spaces ─────────────────────────────┐
│ ●  herdr                             │
│    main  +2 ~1                       │
│    ▕█████▏    ▏ 51.2%                │   $cpu_warn   (amber)
│    ▕███▋      ▏ 2885/7987MB          │   $mem_ok
│    ▕██▋       ▏ 2.11 2.35 2.44       │   $load_ok
│    ▁▂▃▅▆▇▆▄▃▂                        │   $cpu_history
│                                      │
│ ○  dotfiles                          │
│    ▕████████▉ ▏ 89.4%                │   $cpu_hot    (red)
│    ▕███▋      ▏ 2885/7987MB          │   $mem_ok
│    ▕██▋       ▏ 2.11 2.35 2.44       │   $load_ok
└──────────────────────────────────────┘
```

And on a tmux status line, the same sample as one row:

```
2885/7987MB [|||||     ]  51.2% 2.11 2.35 2.44

 ^    ^          ^         ^     ^    ^    ^
 |    |          |         |     |    |    |
 1    2          3         4     5    6    7
```

1. Currently used memory.
2. Available memory.
3. CPU usage bar graph.
4. CPU usage percentage.
5. Load average for the past minute.
6. Load average for the past 5 minutes.
7. Load average for the past 15 minutes.

## Install

Requires Rust 1.85 or newer, and herdr 0.8.0 or newer for the plugin side.
There is no C toolchain and no `sysinfo`-style dependency; every backend goes
straight to the operating system.

From GitHub:

```sh
herdr plugin install thewtex/herdr-mem-cpu-load
```

For local development, link the working tree instead. `plugin link` does not
run the manifest's build commands, so build the release binary that the startup
hook points at first:

```sh
cargo build --release
herdr plugin link /path/to/herdr-mem-cpu-load
herdr plugin list
```

Startup hooks only run when a herdr *server* starts — not when a client
attaches, the config reloads, or a plugin is linked. After linking, either
restart herdr or start the daemon once by hand:

```sh
herdr-mem-cpu-load --daemon &
```

For tmux alone, or for a shell, the binary needs nothing else:

```sh
cargo build --release
cp target/release/herdr-mem-cpu-load ~/.local/bin/
```

## Sidebar layouts

Three recipes for `~/.config/herdr/config.toml`. All three assume the daemon is
running.

### Multi-row with level colours

A row lists all three level tokens for a metric because only one of them ever
has a value: the daemon sets the level that applies and clears the other two,
and herdr drops a token without a value along with its separator. The result is
one row that changes colour as the machine heats up.

```toml
[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace"],
  ["branch", "git_status"],
  [{ token = "$cpu_ok" }, { token = "$cpu_warn", fg = "#f9e2af" }, { token = "$cpu_hot", fg = "#f38ba8" }],
  [{ token = "$mem_ok" }, { token = "$mem_warn", fg = "#f9e2af" }, { token = "$mem_hot", fg = "#f38ba8" }],
  [{ token = "$load_ok" }, { token = "$load_warn", fg = "#f9e2af" }, { token = "$load_hot", fg = "#f38ba8" }],
]
```

### Compact single row

One row carries memory, CPU, and load together:

```toml
[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace"],
  ["$sys_status"],
]
```

Pair it with a narrower CPU graph so the row fits a slim sidebar:

```toml
# config.toml in the plugin's config directory
graph_style = "vertical"
```

### History sparkline

Show where the CPU has been next to where it is now:

```toml
[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace"],
  ["$cpu_status"],
  [{ token = "$cpu_history", fg = "#89b4fa" }],
]
```

`$cpu_history` keeps `--history` samples, which defaults to `--graph-lines`.
Raise it for a longer trace:

```toml
# config.toml in the plugin's config directory
history = 30
```

## Tokens reference

Every report mentions all fourteen keys, setting the ones that apply and
clearing the rest. Example values are for a machine at 51.2% CPU with
2885 MB of 7987 MB used and load averages of 2.11, 2.35, and 2.44.

| Token | Example | When it is set |
| --- | --- | --- |
| `$cpu_status` | `▕█████▏    ▏ 51.2%` | Always. |
| `$mem_status` | `▕███▋      ▏ 2885/7987MB` | Always. |
| `$load_status` | `▕██▋       ▏ 2.11 2.35 2.44` | Unless `averages_count = 0`. |
| `$sys_status` | `2885/7987MB▕█████▏    ▏  51.2% 2.11 2.35 2.44` | Always. The whole one-line status. |
| `$cpu_history` | `▁▂▃▅▆▇▆▄▃▂` | Once the daemon has a sample; cleared when `history = 0`. |
| `$cpu_ok` | `▕█████▏    ▏ 51.2%` | CPU below `cpu_warn`. Same value as `$cpu_status`. |
| `$cpu_warn` | *as above* | CPU at or above `cpu_warn`. |
| `$cpu_hot` | *as above* | CPU at or above `cpu_hot`. |
| `$mem_ok` | `▕███▋      ▏ 2885/7987MB` | Memory below `mem_warn`. Same value as `$mem_status`. |
| `$mem_warn` | *as above* | Memory at or above `mem_warn`. |
| `$mem_hot` | *as above* | Memory at or above `mem_hot`. |
| `$load_ok` | `▕██▋       ▏ 2.11 2.35 2.44` | One minute load per core below `load_warn`. |
| `$load_warn` | *as above* | Load per core at or above `load_warn`. |
| `$load_hot` | *as above* | Load per core at or above `load_hot`. |

Exactly one level token per metric ever carries a value. A level rises the
moment a value reaches its threshold and only falls back once the value is
five percentage points — a tenth of a core, for load — below it again, so a
machine hovering on a threshold does not flicker between two colours.

Values are capped at herdr's 80 character limit and contain no control
characters, so a token is safe to place anywhere in a row. When a wide bar and
a long reading will not both fit, the bar is narrowed rather than the value
truncated — a cell of an approximate graph is worth less than the number at the
end of the row. `$sys_status` is what binds: it carries the memory, CPU, and
load segments together.

## Configuration

herdr gives every plugin a private config directory:

```sh
herdr plugin config-dir thewtex.mem-cpu-load
```

Drop a commented `config.toml` in it, either by hand or with the binary:

```sh
herdr-mem-cpu-load --write-default-config
```

which is also what the `write-config` plugin action does. Check what the merge
produced with:

```sh
herdr-mem-cpu-load --print-config
```

### Resolution order

Highest wins:

1. explicit command line flags,
2. the file named by `--config <PATH>`,
3. `$HERDR_PLUGIN_CONFIG_DIR/config.toml`,
4. the built-in defaults.

A missing file is not an error. A file with an unknown key or an out-of-range
value is reported on stderr, naming the file and the line, and then ignored, so
a typo cannot silently kill the sidebar.

The daemon watches the file it resolved its settings from and re-runs that
merge whenever it changes, so an edit takes effect on the next tick rather than
the next restart. Every key below can be changed under a running daemon. The
command line still wins: a flag passed at startup cannot be taken away by
editing the file underneath it. A file that stops parsing mid-save leaves the
daemon on the settings it already has, and deleting the file falls back to the
command line over the defaults.

### Keys

| Key | Default | Meaning |
| --- | --- | --- |
| `interval_secs` | `1` | Seconds between samples; also the CPU measurement window. 1 to 3600. |
| `ttl_ms` | `interval_secs × 2000 + 1000` | How long herdr keeps the tokens without a refresh. 1 to 86400000, herdr's accepted range. Daemon only. |
| `source` | `"system-monitor"` | The metadata source the tokens are reported under. Daemon only. |
| `workspaces` | `"all"` | Which workspaces the rows appear under: `"all"`, or `"focused"` for the active one alone. Daemon only. |
| `graph_style` | `"classic"`, `"blocks"` with `--daemon` | `"classic"`, `"blocks"`, or `"vertical"`. |
| `graph_lines` | `10` | Cells in the CPU graph. `0` hides it, `64` is the maximum. |
| `mem_graph_lines` | `graph_lines` | Cells in the memory bar. |
| `load_graph_lines` | `graph_lines` | Cells in the load bar, which draws the one minute load per core. |
| `mem_mode` | `0` | `0`: used/total, `1`: free memory, `2`: usage percent. |
| `cpu_mode` | `0` | `0`: max 100%, `1`: max 100% per thread. |
| `averages_count` | `3` | How many load averages to print, `0` to `3`. |
| `history` | `graph_lines` | Samples kept for `$cpu_history`. Daemon only. |
| `verbose` | `false` | Log every tick's status line, not just errors. Daemon only. |
| `thresholds.cpu_warn` | `50.0` | CPU percentage that turns `$cpu_ok` into `$cpu_warn`. |
| `thresholds.cpu_hot` | `80.0` | CPU percentage that turns it into `$cpu_hot`. |
| `thresholds.mem_warn` | `70.0` | Used-memory percentage for `$mem_warn`. |
| `thresholds.mem_hot` | `90.0` | Used-memory percentage for `$mem_hot`. |
| `thresholds.load_warn` | `0.7` | One minute load per core for `$load_warn`. |
| `thresholds.load_hot` | `1.0` | One minute load per core for `$load_hot`. `1.0` is exactly saturated. |

A complete file, all defaults spelled out:

```toml
interval_secs = 1
ttl_ms = 3000
source = "system-monitor"
workspaces = "all"
graph_style = "classic"
graph_lines = 10
mem_graph_lines = 10
load_graph_lines = 10
mem_mode = 0
cpu_mode = 0
averages_count = 3
history = 10
verbose = false

[thresholds]
cpu_warn = 50.0
cpu_hot = 80.0
mem_warn = 70.0
mem_hot = 90.0
load_warn = 0.7
load_hot = 1.0
```

Nothing here has to be set. A file that only narrows the memory and load bars
is a complete file:

```toml
mem_graph_lines = 4
load_graph_lines = 4
```

Each of the three bars can be a different width; `mem_graph_lines` and
`load_graph_lines` both fall back to `graph_lines` when they are not set, so
setting `graph_lines` alone still moves all three together.

## One-line mode

```sh
herdr-mem-cpu-load [OPTIONS]
```

| Flag | Default | Description |
| --- | --- | --- |
| `-i`, `--interval <SECS>` | `1` | Status refresh interval in seconds; also the CPU sampling window. 1 to 3600. |
| `-g`, `--graph-lines <N>` | `10` | Cells in the CPU graph. `0` hides the graph, `64` is the maximum. |
| `--mem-graph-lines <N>` | `--graph-lines` | Cells in the memory bar. Daemon mode only; one-line mode draws no memory bar. |
| `--load-graph-lines <N>` | `--graph-lines` | Cells in the load bar. Daemon mode only; one-line mode draws no load bar. |
| `-m`, `--mem-mode <0\|1\|2>` | `0` | `0`: used/total, `1`: free memory, `2`: usage percent. |
| `-t`, `--cpu-mode <0\|1>` | `0` | `0`: max 100%, `1`: max 100% per thread. |
| `-a`, `--averages-count <0-3>` | `3` | How many load averages to print. |
| `-v`, `--vertical-graph` | off | Single-character vertical bar chart for the CPU graph. |
| `--graph-style <STYLE>` | `classic`, `blocks` in `--daemon` | `classic` (`[\|\|\|\|\|     ]`), `blocks` (unicode eighths), or `vertical`. |
| `-c`, `--colors` | off | Wrap each segment in tmux colour markup. |
| `-p`, `--powerline-left` | off | Left-pointing powerline separators. Implies `--colors`. |
| `-q`, `--powerline-right` | off | Right-pointing powerline separators. Implies `--colors`. |
| `-l`, `--segments-left <COLOR>` | – | Blend the first segment with this tmux colour (0-255). |
| `-r`, `--segments-right <COLOR>` | – | Blend the last segment with this tmux colour (0-255). |
| `--ansi` | off | 256-colour ANSI escapes instead of tmux markup. |
| `--watch` | off | Re-sample every interval and reprint the line in place. |
| `--config <PATH>` | – | Read this configuration file instead of the plugin's. |
| `--print-config` | off | Print the effective configuration as TOML and exit. |
| `--write-default-config` | off | Write a commented `config.toml` and exit. |
| `--force` | off | Let `--write-default-config` overwrite an existing file. |
| `--daemon` | off | Run as a herdr plugin daemon instead of printing one line. |
| `--ttl-ms <N>` | `interval × 2 + 1000` | How long herdr keeps the reported tokens. Daemon mode only. |
| `--source <ID>` | `system-monitor` | Metadata source the tokens are reported under. Daemon mode only. |
| `--workspaces <all\|focused>` | `all` | Report to every open workspace, or to the active one alone. Daemon mode only. |
| `--history <N>` | `--graph-lines` | Samples kept for the `$cpu_history` sparkline. Daemon mode only. |
| `--log-file <PATH>` | – | Append daemon diagnostics here. Daemon mode only. |
| `--verbose` | off | Log every tick's status line, not just errors. Daemon mode only. |

### Graph styles

```
classic    [|||||     ]  51.2%
blocks    ▕█████▏    ▏  51.2%
vertical  ▕▅▏  51.2%
```

### tmux

```tmux
set -g status-interval 2
set -g status-left "#S #[fg=green,bg=black]#(herdr-mem-cpu-load --colors --interval 2)#[default]"
set -g status-left-length 60
```

The `--interval` argument should be the same number of seconds
`status-interval` is set to.

### Colours

`--colors` emits the original's tmux markup: a `#[fg=...,bg=colour...]` before
each segment and a `#[fg=default,bg=default]` after it, with the background
taken from the same lookup tables `tmux-mem-cpu-load` uses. The powerline flags
draw the `` and `` separators between segments, and `-l`/`-r` blend
the ends with whatever tmux draws either side:

```tmux
set -g status-right '#(herdr-mem-cpu-load --powerline-right --segments-right 235 --interval 2)'
```

`--ansi` emits the equivalent 256-colour SGR escapes for a plain terminal:

```sh
herdr-mem-cpu-load --ansi
```

Without either flag the output is byte for byte what the uncoloured original
prints, so a pipe still reads it as text.

### Watch mode

```sh
herdr-mem-cpu-load --watch --interval 2
```

re-samples every interval and rewrites the line in place — a carriage return
and an erase-to-end-of-line, not a full-screen TUI. Colour is on by default
when stdout is a terminal and off when it is redirected. It stops on Ctrl-C,
on `q`, or when its terminal closes.

This is what the plugin's popup pane runs:

```sh
herdr plugin pane open --plugin thewtex.mem-cpu-load --entrypoint status
```

Bind it, or one of the two plugin actions, to a key in
`~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+s"
type = "plugin_action"
command = "thewtex.mem-cpu-load.show-status"
```

`herdr plugin action list --plugin thewtex.mem-cpu-load` lists the actions:
`show-status` prints the effective configuration into the plugin log, and
`write-config` writes a commented `config.toml` into the plugin config
directory.

## Platforms

Every backend goes straight to the operating system; there is no
`sysinfo`-style dependency in between.

| Platform | CPU | Memory | Load average |
| --- | --- | --- | --- |
| Linux | `/proc/stat` | `/proc/meminfo` | `getloadavg` |
| macOS | `host_statistics(HOST_CPU_LOAD_INFO)` | `hw.memsize` and `host_statistics64(HOST_VM_INFO64)` | `getloadavg` |
| Windows | `GetSystemTimes` | `GlobalMemoryStatusEx` | emulated (see below) |
| FreeBSD, OpenBSD, NetBSD | – | – | – |

macOS counts active plus wired pages as used memory, the same formula
`tmux-mem-cpu-load` uses. The compressed pool is deliberately left out, so the
number matches the original rather than Activity Monitor.

### The Windows load average

Windows has no load average, and the original Windows port simply printed
nothing where the three numbers go. This one emulates them with the same
exponentially weighted moving average the Linux kernel uses. For each window
`T` of 60, 300, and 900 seconds, and a gap of `dt` seconds since the previous
sample:

```
factor = exp(-dt / T)
load   = load * factor + busy_cores * (1 - factor)
```

`busy_cores` is `cpu_percent / 100 * cpu_count` — the same unit a real load
average is in. The first sample seeds all three windows instead of decaying up
from zero, so the first reading is useful rather than 0.

The averages therefore only mean something in `--daemon` and `--watch` mode,
where the emulator accumulates history tick after tick. One-line mode takes a
single measurement and exits, so it has nothing to decay: it prints the current
busy core count three times. Expect the three numbers to be identical there.

### The BSDs

FreeBSD, OpenBSD, and NetBSD compile and run, but the sampling functions report
`unsupported platform` rather than numbers. Contributions are welcome: the
porting reference is `freebsd/`, `openbsd/`, and `netbsd/` in
[tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load) — `cpu.cc` and
`memory.cc` in each. A backend is four functions (`cpu_times`,
`memory_status`, `load_averages`, `cpu_count`); add `src/sys/<os>.rs`, wire the
`cfg` arm in `src/sys/mod.rs`, and re-export `load_averages` and `cpu_count`
from `src/sys/unix_common.rs` if `getloadavg` and `sysconf` do the right thing
there.

## Troubleshooting

**Nothing appears in the sidebar.** Check that the daemon is running and what
it said:

```sh
herdr plugin log list --plugin thewtex.mem-cpu-load
```

The daemon itself writes nothing to stdout — herdr copies a plugin's output
into a capped command log, and a line every two seconds would fill it within
the hour. Diagnostics go to `daemon.log` in the plugin's state directory
instead, which is `$HERDR_PLUGIN_STATE_DIR` when herdr started the daemon:

```sh
tail -f ~/.local/state/herdr/plugins/thewtex.mem-cpu-load/daemon.log
```

macOS uses the same path; Windows uses
`%LOCALAPPDATA%\herdr\plugins\thewtex.mem-cpu-load\daemon.log`. A daemon
started by hand has no state directory, so give it a path:

```sh
herdr-mem-cpu-load --daemon --log-file /tmp/mem-cpu-load.log &
```

Add `--verbose` (or `verbose = true`) to log every tick's status line, not just
errors.

**The same rows repeat under every Space.** They are one machine's numbers, so
by default every open workspace gets a copy. Report them to the active
workspace instead:

```toml
workspaces = "focused"
```

The rows follow the focus: the workspace being left has its tokens cleared on
the same tick the new one is given them, rather than keeping them until the
TTL runs out. It is also one `herdr` call per tick instead of one per
workspace. If herdr answers with no focused workspace at all — which happens
while the focus is moving — the rows stay where they last were rather than
blinking out.

**Rows appear and then vanish.** The tokens have a TTL: herdr drops them if the
daemon stops refreshing. The default is two intervals plus a second of slack,
so a stopped daemon's rows disappear on their own within a few seconds. If the
machine is loaded enough that a tick regularly runs long, raise `ttl_ms`.

**Nothing happened after `herdr plugin link`.** Startup hooks only run when a
herdr *server* starts. Restart herdr, or start the daemon by hand once:

```sh
herdr-mem-cpu-load --daemon &
```

**A second daemon exits immediately.** That is the point. Only one daemon runs
at a time: it takes an advisory lock on
`$HERDR_PLUGIN_STATE_DIR/herdr-mem-cpu-load.lock` (the system temporary
directory for a daemon started by hand) and a second instance logs
`another herdr-mem-cpu-load daemon is already running` and exits 0. The lock
file holds the running daemon's pid. The kernel releases the lock when the
process exits or is killed, so there is nothing to clean up after a crash.
This is what keeps a herdr live handoff — which re-runs `[[startup]]` — from
doubling up.

**A configuration change did nothing.** Confirm what the merge produced:

```sh
herdr-mem-cpu-load --config /path/to/config.toml --print-config
```

A malformed file is reported on stderr and then ignored; look for
`herdr-mem-cpu-load: ignoring ...` in the plugin log. The daemon re-reads the
file when it changes, so an edit lands within one interval and no restart is
needed; `configuration reloaded: ...` in the daemon log is the confirmation.
One-line and `--watch` mode read the file once, when they start.

## Publishing to the herdr marketplace

The [marketplace](https://herdr.dev/plugins/) indexes public GitHub
repositories automatically; nothing is submitted and nothing is reviewed. A
repository is listed when both of these are true:

1. it carries the GitHub topic `herdr-plugin`, and
2. its default branch contains at least one `herdr-plugin.toml` whose required
   metadata parses — `id`, `name`, `version`, and `min_herdr_version`.

This repository keeps its manifest at the root, which is also where
`herdr plugin install thewtex/herdr-mem-cpu-load` looks. Manifests in
subdirectories are indexed too, and each one is listed as a separately
installable plugin under a single repository card.

The index refreshes every 30 minutes and rescans a repository when its
default-branch head moves, so a release shows up on its own. Forks, archived
repositories, and repositories whose manifest metadata is malformed are
excluded — which is why `scripts/check_manifest.py` runs in CI, and why
`scripts/release.sh` is the only supported way to bump a version: a manifest
that stops parsing, or a `version` that has drifted from `Cargo.toml`, silently
drops the plugin off the listing.

A card shows the repository name, description, star count, primary language,
and last push, plus each manifest's `name` and `version`. See herdr's
[marketplace documentation](https://herdr.dev/docs/marketplace/) for the full
rules.

## Contributing

Build, test, cross-target checks, how to run this against a live herdr, and the
release process are in [CONTRIBUTING.md](CONTRIBUTING.md). Changes are logged in
[CHANGELOG.md](CHANGELOG.md).

## Credits

Re-imagined from [tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load)
by Matt McCormick and contributors. The colour lookup tables, the powerline
separators, and the one-line output format are ports of that project's
`common/luts.h`, `common/powerline.cc`, and `common/main.cc`.

## License

Apache License 2.0. See [LICENSE](LICENSE).
