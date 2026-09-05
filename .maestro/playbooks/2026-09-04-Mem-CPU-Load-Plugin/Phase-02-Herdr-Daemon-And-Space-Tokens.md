# Phase 02: Herdr Daemon Mode and Space Sidebar Tokens

This phase makes the binary a real herdr plugin. A `--daemon` mode samples on an interval, discovers every open workspace through the herdr CLI, and reports composable Space sidebar tokens (`$cpu_status`, `$mem_status`, `$load_status`, `$sys_status`, `$cpu_history`, plus level tokens such as `$cpu_hot` that only exist past a threshold so users can style them with `fg`). The manifest gains `[[startup]]` hooks so herdr launches the daemon automatically, and the README documents a copy-paste `[ui.sidebar.spaces]` layout. By the end, linking the plugin and starting the daemon shows live CPU, memory, and load rows under every workspace in the herdr sidebar.

## Tasks

- [ ] Build a small herdr client in `src/herdr/mod.rs` and `src/herdr/cli.rs` that talks to herdr by spawning the CLI (portable across Unix sockets and Windows named pipes, as recommended in `/home/matt/src/herdr/docs/next/website/src/content/docs/plugins.mdx`):
  - `pub struct HerdrCli { bin: PathBuf }` constructed from `HERDR_BIN_PATH` when set, else `herdr` resolved on `PATH`.
  - `pub fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, HerdrError>` running `workspace list`, parsing stdout JSON of the shape `{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"workspace_id":"w1","label":"~","number":1,"focused":false,...}]}}` with serde (only `workspace_id`, `label`, `number`, `focused` are needed; ignore unknown fields). A non-zero exit, unparsable output, or a top-level `error` object becomes `HerdrError`.
  - `pub struct MetadataReport { pub workspace_id: String, pub source: String, pub set: Vec<(String, String)>, pub clear: Vec<String>, pub ttl_ms: u64, pub seq: u64 }` and `pub fn report_metadata_args(report: &MetadataReport) -> Vec<String>` (pure, unit-testable) producing `["workspace", "report-metadata", "<id>", "--source", "<source>", "--token", "name=value", ..., "--clear-token", "name", ..., "--ttl-ms", "<n>", "--seq", "<n>"]`, and `pub fn report_metadata(&self, report: &MetadataReport) -> Result<(), HerdrError>` that runs it with stdout and stderr captured (never inherited) and treats non-zero exit as an error.
  - Both commands must have a timeout guard: spawn with piped output and use `std::process::Child::try_wait` in a loop with a 10 second budget, killing the child on timeout, so a hung herdr cannot freeze the sampler. Keep the implementation dependency-free (no tokio).
  - `pub enum HerdrError { Spawn(io::Error), NonZeroExit { code: Option<i32>, stderr: String }, Parse(String), Timeout }` with `Display`.

- [ ] Implement token generation in `src/tokens.rs` as a pure function from a `Sample` (plus history) to a `TokenSet`, so it is fully unit-testable and independent of herdr:
  - `pub struct Thresholds { pub cpu_warn: f32, pub cpu_hot: f32, pub mem_warn: f32, pub mem_hot: f32, pub load_warn: f64, pub load_hot: f64 }` with `Default` = cpu 50/80 percent, mem 70/90 percent, load 0.7/1.0 (load-per-core, using `metrics::load::load_per_core`).
  - `pub enum Level { Ok, Warn, Hot }` and `pub fn classify(value: f64, warn: f64, hot: f64) -> Level` (hot when ≥ hot, warn when ≥ warn, else ok).
  - `pub struct TokenOptions { pub graph_style: GraphStyle, pub graph_lines: usize, pub mem_mode: MemoryMode, pub cpu_mode: CpuMode, pub averages_count: u8, pub thresholds: Thresholds }` with `Default` = (Blocks, 10, Default, Default, 3, Thresholds::default()). Note the daemon defaults to `Blocks` while one-line mode defaults to `Classic`.
  - `pub struct TokenSet { pub set: Vec<(String, String)>, pub clear: Vec<String> }` and `pub fn build_tokens(sample: &Sample, history: &[f32], opts: &TokenOptions) -> TokenSet` producing exactly these keys:
    - `cpu_status` = `render_bar(style, cpu_percent, lines)` + space + trimmed `cpu_text` (e.g. `▕█████▌    ▏ 51.2%`).
    - `mem_status` = `render_bar(style, used_percent, lines)` + space + `mem_text` (e.g. `▕███▌      ▏ 2885/7987MB`).
    - `load_status` = `render_bar(style, min(load_per_core*100, 100), lines)` + space + trimmed `load_text` (e.g. `▕██▏       ▏ 2.11 2.35 2.44`); when `averages_count` is 0 the token is cleared instead of set.
    - `sys_status` = the same string as `render::format::status_line` using the token options (one combined line for single-row layouts).
    - `cpu_history` = `sparkline(history)` when history is non-empty, otherwise cleared.
    - Level tokens for each metric `cpu`, `mem`, `load`: `<metric>_ok`, `<metric>_warn`, `<metric>_hot`. Exactly one of the three is set to the same text as `<metric>_status`; the other two are listed in `clear`. This lets a single row like `[{ token = "$cpu_ok" }, { token = "$cpu_warn", fg = "#f9e2af" }, { token = "$cpu_hot", fg = "#f38ba8" }]` change color by level because herdr drops tokens without values and their separators.
  - Enforce herdr limits in this function with debug assertions and a unit test: at most 16 keys mentioned per report (this design mentions 14: 5 status/history keys plus 9 level keys), every value ≤ 80 characters (herdr counts `char`s, not bytes, so block characters count as one), no control characters, and keys matching `[A-Za-z0-9_-]{1,32}`.
  - Add `pub fn truncate_chars(value: &str, max: usize) -> String` used defensively on every value.

- [ ] Implement the daemon loop in `src/daemon.rs` and expose it through a `--daemon` flag in `src/cli.rs` (one-line mode remains the default when the flag is absent):
  - `pub struct DaemonOptions { pub interval: Duration, pub ttl_ms: u64, pub source: String, pub tokens: TokenOptions, pub history_len: usize, pub max_failures: u32, pub log: Option<PathBuf> }`. Defaults: interval 2 s, `ttl_ms = interval_ms * 2 + 1000` (5000 ms for the default interval, matching the `--ttl-ms 5000` example), source `system-monitor`, history length = `graph_lines`, `max_failures` 5. CLI flags: `--daemon`, `--ttl-ms <N>` (override), `--source <ID>`, `--history <N>`, `--log-file <PATH>`. `--interval` is shared with one-line mode.
  - Loop: keep a `CpuSampler` so each tick uses the delta since the previous tick (no sleeping inside the sample). On the first tick, take the baseline, sleep one interval, and continue. Each tick: sample, `metrics::collect`, push the CPU percent into a fixed-size history ring (`VecDeque<f32>` capped at `history_len`), `build_tokens`, `list_workspaces`, then `report_metadata` for every workspace. Sleep for the remainder of the interval measured from the tick start so reporting cost does not drift the schedule.
  - `seq` must be monotonic across daemon restarts because herdr ignores reports whose `--seq` is ≤ the last accepted value for the same source. Use the current Unix time in milliseconds as the sequence number, and never reuse a value within a process (bump by one if the clock did not advance).
  - Exit rules: stop with exit code 0 when `list_workspaces` fails `max_failures` times in a row (herdr is gone), or when `HERDR_SOCKET_PATH` is set to a path that no longer exists on Unix (check each tick; skip this check on Windows where the value is a named pipe). Individual `report_metadata` failures for one workspace (for example it closed mid-tick) are logged and skipped, not counted as fatal.
  - Output discipline: herdr captures plugin stdout/stderr into a 64 KiB command log, so the daemon must not print per tick. Write nothing to stdout; write a single startup line and errors to a log file when `--log-file` is given or `HERDR_PLUGIN_STATE_DIR` is set (`<state dir>/daemon.log`, appended, with a timestamp prefix). Add `--verbose` to also log each tick's `sys_status` line.
  - The daemon must ignore `SIGPIPE`-style write failures (use `let _ =` on log writes) and must never panic on a sampling error; log it and continue to the next tick.

- [ ] Update the plugin manifest and README for daemon mode:
  - `herdr-plugin.toml`: add `[[startup]]` with `command = ["target/release/herdr-mem-cpu-load", "--daemon"]` and `platforms = ["linux", "macos"]`, and a second `[[startup]]` with `command = ["target/release/herdr-mem-cpu-load.exe", "--daemon"]` and `platforms = ["windows"]`. herdr resolves relative program paths that contain a separator against the plugin root (see `program_for_cwd` in `/home/matt/src/herdr/src/plugin_command.rs`), so no launcher script is needed. Keep the existing `[[build]]`.
  - `README.md`: add an "Install" section (`herdr plugin install thewtex/herdr-mem-cpu-load` and the local `herdr plugin link` flow, noting that `plugin link` does not run build commands so local users run `cargo build --release` first), a "Sidebar layout" section with this example and a table of every token the daemon reports:

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

    plus a compact single-row alternative using `["$sys_status"]` and a history example `["$cpu_history", "$cpu_status"]`. Explain that the daemon reports the same machine-wide values to every workspace with a TTL slightly above the interval so rows disappear automatically when the daemon stops, and that startup hooks run only when a herdr server starts, so after `plugin link` the user either restarts herdr or runs `herdr-mem-cpu-load --daemon &` once by hand.

- [ ] Write unit tests for the new modules (`#[cfg(test)]` in each file, plus fixtures inline as string literals):
  - `herdr/cli.rs`: parse the real `workspace list` payload shape shown above with two workspaces and unknown extra fields; an `{"error":{...}}` payload becomes `HerdrError::Parse`; `report_metadata_args` produces the exact argv order and pairs for a report with two set tokens, two cleared tokens, ttl 5000, seq 42.
  - `tokens.rs`: default thresholds classify 10/60/95 percent CPU as Ok/Warn/Hot; `build_tokens` on a synthetic `Sample` sets `cpu_status`, `mem_status`, `load_status`, `sys_status`, exactly one `cpu_*` level token and clears the other two (same for mem and load); `cpu_history` is cleared for empty history and equals `sparkline` otherwise; the total of `set.len() + clear.len()` is ≤ 16; every value is ≤ 80 chars; `averages_count = 0` clears `load_status`; `truncate_chars` cuts on character boundaries for a string of block characters.
  - `daemon.rs`: extract the sequence generator into `SeqGenerator` and test that consecutive calls are strictly increasing even when the clock is frozen (inject a clock closure); test the history ring keeps only the last `history_len` values.

- [ ] Run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`; fix all failures.

- [ ] Verify against the running herdr server on this machine (a server is running: `herdr workspace list` returns JSON) and then commit:
  - `cargo build --release`.
  - Start the daemon in the background with a short interval and a temporary log: `./target/release/herdr-mem-cpu-load --daemon --interval 1 --verbose --log-file /tmp/herdr-mem-cpu-load.log & echo $! > /tmp/hmcl.pid`, wait 4 seconds, then run `herdr workspace list` and confirm the JSON now contains a `tokens` map with `cpu_status`, `mem_status`, `load_status`, `sys_status`, and exactly one of `cpu_ok`/`cpu_warn`/`cpu_hot` per workspace. Also `cat /tmp/herdr-mem-cpu-load.log` to confirm ticks are logged and no errors appear.
  - Stop the daemon (`kill $(cat /tmp/hmcl.pid)`), wait 6 seconds, and confirm the tokens have expired from `herdr workspace list` (TTL behaviour).
  - Run `herdr plugin link /home/matt/src/herdr-mem-cpu-load` and `herdr plugin list --json` to confirm the manifest with startup hooks validates, then leave the plugin linked (the user wants it installed).
  - Commit with the message `feat: add herdr daemon mode reporting Space sidebar tokens`.
