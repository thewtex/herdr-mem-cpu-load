# herdr-mem-cpu-load

CPU, memory, and load average monitor for [herdr](https://github.com/thewtex/herdr),
re-imagined in Rust from [tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load).

The memory monitor displays used and available memory. The CPU monitor reports
a percentage across all processors along with a bar graph of the current usage.
The system load averages round out the line.

The binary is a drop-in replacement for `tmux-mem-cpu-load` in a tmux status
line, and it is also the sampling core of the herdr plugin of the same name.

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
| `--graph-style <STYLE>` | `classic` | `classic` (`[\|\|\|\|\|     ]`), `blocks` (unicode eighths), or `vertical`. |
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

`herdr-plugin.toml` declares the plugin metadata and the release build command.
Daemon mode — the long-running process that feeds the herdr Space sidebar — is
coming in the next phase; today the manifest has no `[[startup]]` entry and the
binary only prints a single line and exits.

## Credits and license

Ported from [tmux-mem-cpu-load](https://github.com/thewtex/tmux-mem-cpu-load)
by Matthew McCormick, Pawel 'l0ner' Soltys, and contributors. Like the
original, this project is licensed under the Apache License, Version 2.0. See
[LICENSE](LICENSE).
