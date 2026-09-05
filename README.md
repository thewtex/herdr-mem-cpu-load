# herdr-mem-cpu-load

CPU, memory, and load average monitor for [herdr](https://github.com/thewtex/herdr),
re-imagined in Rust from [tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load).

The memory monitor displays used and available memory. The CPU monitor reports
a percentage across all processors along with a bar graph of the current usage.
The system load averages round out the line.

The binary is a drop-in replacement for `tmux-mem-cpu-load` in a tmux status
line, and with `--daemon` it is the herdr plugin of the same name, feeding live
CPU, memory, and load rows to every workspace in the herdr Space sidebar.

## Example output

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

## Building

Requires Rust 1.85 or newer.

```sh
cargo build --release
```

The binary lands at `target/release/herdr-mem-cpu-load`.

## Usage

```sh
herdr-mem-cpu-load [OPTIONS]
```

| Flag | Default | Description |
| --- | --- | --- |
| `-i`, `--interval <SECS>` | `1` | Status refresh interval in seconds; also the CPU sampling window. Must be at least 1. |
| `-g`, `--graph-lines <N>` | `10` | Cells in the CPU graph. `0` hides the graph. |
| `-m`, `--mem-mode <0\|1\|2>` | `0` | `0`: used/total, `1`: free memory, `2`: usage percent. |
| `-t`, `--cpu-mode <0\|1>` | `0` | `0`: max 100%, `1`: max 100% per thread. |
| `-a`, `--averages-count <0-3>` | `3` | How many load averages to print. |
| `-v`, `--vertical-graph` | off | Single-character vertical bar chart for the CPU graph. |
| `--graph-style <STYLE>` | `classic`, `blocks` in `--daemon` | `classic` (`[\|\|\|\|\|     ]`), `blocks` (unicode eighths), or `vertical`. |
| `--daemon` | off | Run as a herdr plugin daemon instead of printing one line. |
| `--ttl-ms <N>` | `interval x 2 + 1000` | How long herdr keeps the reported tokens. Daemon mode only. |
| `--source <ID>` | `system-monitor` | Metadata source the tokens are reported under. Daemon mode only. |
| `--history <N>` | `--graph-lines` | Samples kept for the `$cpu_history` sparkline. Daemon mode only. |
| `--log-file <PATH>` | – | Append daemon diagnostics here. Daemon mode only. |
| `--verbose` | off | Log every tick's status line, not just errors. Daemon mode only. |
| `-c`, `--colors` | off | Accepted and ignored; reserved for Phase 04. |
| `-p`, `--powerline-left` | off | Accepted and ignored; reserved for Phase 04. |
| `-q`, `--powerline-right` | off | Accepted and ignored; reserved for Phase 04. |
| `-l`, `--segments-left <COLOR>` | – | Accepted and ignored; reserved for Phase 04. |
| `-r`, `--segments-right <COLOR>` | – | Accepted and ignored; reserved for Phase 04. |

The compatibility flags are parsed but have no effect yet, so an existing
`tmux.conf` line keeps working unchanged.

### Graph styles

```
classic    [|||||     ]  51.2%
blocks    ▕█████     ▏  51.2%
vertical  ▕▅▏  51.2%
```

### tmux

```tmux
set -g status-interval 2
set -g status-left "#S #(herdr-mem-cpu-load --interval 2)"
set -g status-left-length 60
```

## herdr plugin

With `--daemon` the binary stops being a one-shot command and becomes a
sampler: every interval it reads the machine's CPU, memory, and load, asks
herdr which workspaces are open, and reports a set of Space sidebar tokens to
each of them. Add the tokens you want to `[ui.sidebar.spaces]` and every
workspace grows live system rows.

### Install

```sh
herdr plugin install thewtex/herdr-mem-cpu-load
```

For local development, link the working tree instead. `plugin link` does not
run the manifest's build commands, so build the release binary the startup hook
points at first:

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

### Sidebar layout

```toml
[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace"],
  ["branch", "git_status"],
  [{ token = "$cpu_ok" }, { token = "$cpu_warn", fg = "#f9e2af" }, { token = "$cpu_hot", fg = "#f38ba8" }],
  [{ token = "$mem_ok" }, { token = "$mem_warn", fg = "#f9e2af" }, { token = "$mem_hot", fg = "#f38ba8" }],
  ["$load_status"],
]
```

A row lists all three level tokens because only one of them ever has a value:
the daemon sets the level that applies and clears the other two, and herdr
drops a token without a value along with its separator. The result is one row
that changes colour as the machine heats up.

For a compact sidebar, one row carries everything:

```toml
[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace"],
  ["$sys_status"],
]
```

Or pair the recent history with the current value:

```toml
[ui.sidebar.spaces]
rows = [
  ["state_icon", "workspace"],
  ["$cpu_history", "$cpu_status"],
]
```

### Tokens

| Token | Example | Notes |
| --- | --- | --- |
| `$cpu_status` | `▕█████▏    ▏ 51.2%` | CPU bar and percentage. |
| `$mem_status` | `▕███▋      ▏ 2885/7987MB` | Memory bar and the `--mem-mode` text. |
| `$load_status` | `▕██▋       ▏ 2.11 2.35 2.44` | Load bar and averages. Cleared by `--averages-count 0`. |
| `$sys_status` | `2885/7987MB▕█████▏    ▏  51.2% 2.11 2.35 2.44` | The whole one-line status, for single-row layouts. |
| `$cpu_history` | `▁▂▄▆█▅▃▁` | Sparkline over the last `--history` samples. |
| `$cpu_ok`, `$cpu_warn`, `$cpu_hot` | same text as `$cpu_status` | Exactly one is set; the other two are cleared. |
| `$mem_ok`, `$mem_warn`, `$mem_hot` | same text as `$mem_status` | Same, for memory. |
| `$load_ok`, `$load_warn`, `$load_hot` | same text as `$load_status` | Same, for load. All three are cleared with `--averages-count 0`. |

The level thresholds are CPU 50% / 80%, memory 70% / 90%, and load per core
0.70 / 1.00 — the one minute average divided by the CPU count, so `1.00` means
the machine is exactly saturated.

### How the daemon behaves

* The numbers are machine-wide, so every workspace gets the same values. The
  per-workspace report exists because sidebar tokens live on a workspace.
* Tokens are reported with a TTL slightly above the interval (twice the
  interval plus a second). If the daemon stops, the rows expire and disappear
  on their own rather than freezing at the last reading.
* Reports carry a Unix-millisecond `--seq`, so herdr keeps accepting them
  across daemon restarts.
* The daemon never writes to stdout: herdr captures plugin output into a capped
  command log. Pass `--log-file` (or let it use the plugin state directory) to
  see what it is doing, and add `--verbose` to log every tick.
* It exits quietly when herdr goes away — when the socket in
  `HERDR_SOCKET_PATH` disappears, or after five consecutive failed
  `workspace list` calls.

## Credits and license

Ported from [tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load)
by Matthew McCormick, Pawel 'l0ner' Soltys, and contributors. Like the
original, this project is licensed under the Apache License, Version 2.0. See
[LICENSE](LICENSE).
