//! End-to-end tests over the built binary.
//!
//! Everything here spawns `env!("CARGO_BIN_EXE_herdr-mem-cpu-load")` as a real
//! child process, which is the only way to cover argument validation, the exit
//! codes, and the daemon's conversation with herdr. No test-only dependency is
//! pulled in: the Phase 01 verification regex is hand-matched below and the
//! fake herdr is a script the test writes into a temporary directory.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use herdr_mem_cpu_load::config::FileConfig;
use herdr_mem_cpu_load::daemon::DEFAULT_MAX_FAILURES;

/// The binary under test. Cargo builds it before running this target.
const BIN: &str = env!("CARGO_BIN_EXE_herdr-mem-cpu-load");

/// What the fake herdr answers `workspace list` with: two open workspaces,
/// carrying the same extra fields the real CLI sends and this plugin ignores.
const WORKSPACE_LIST: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"w1:t1","focused":false,"label":"~","number":1,"workspace_id":"w1"},{"active_tab_id":"w9:t3","focused":true,"label":"herdr","number":2,"workspace_id":"w9"}]}}"#;

/// The characters [`herdr_mem_cpu_load::render::graph::vertical_char`] can
/// produce, which is what `-v` draws between the two frame characters.
const VERTICAL_GLYPHS: &str =
    " \u{2581}\u{2582}\u{2583}\u{2584}\u{2585}\u{2586}\u{2587}\u{2588}\u{25b2}";
const FRAME_LEFT: char = '\u{2595}';
const FRAME_RIGHT: char = '\u{258f}';

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A directory under the system temporary directory, removed when the test
/// that owns it ends.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "herdr-mem-cpu-load-cli-{}-{name}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("the temporary directory is created");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}

/// The binary, pointed at an empty config and state directory.
///
/// Without this a `config.toml` belonging to the machine running the tests
/// would change what every assertion sees, and two daemon tests would fight
/// over one singleton lock file.
fn command(dir: &TempDir) -> Command {
    let mut command = Command::new(BIN);
    command
        .env("HERDR_PLUGIN_CONFIG_DIR", &dir.path)
        .env("HERDR_PLUGIN_STATE_DIR", &dir.path)
        .env_remove("HERDR_SOCKET_PATH")
        .env_remove("HERDR_BIN_PATH")
        .stdin(Stdio::null());
    command
}

fn run(dir: &TempDir, args: &[&str]) -> Output {
    command(dir)
        .args(args)
        .output()
        .expect("the binary runs to completion")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .trim_end()
        .to_string()
}

/// One successful run's single line of output.
fn one_line(dir: &TempDir, args: &[&str]) -> String {
    let output = run(dir, args);
    assert!(
        output.status.success(),
        "{args:?} exited with {:?}: {}",
        output.status.code(),
        stderr_of(&output)
    );
    let line = stdout_of(&output);
    assert_eq!(line.lines().count(), 1, "expected one line, got {line:?}");
    line
}

// ---------------------------------------------------------------------------
// The Phase 01 verification regex, matched by hand
// ---------------------------------------------------------------------------

fn is_uint(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `\d+\.\d{decimals}`
fn is_fixed(text: &str, decimals: usize) -> bool {
    match text.split_once('.') {
        Some((whole, fraction)) => {
            is_uint(whole) && fraction.len() == decimals && is_uint(fraction)
        }
        None => false,
    }
}

/// `\d+(/\d+MB|MB/\d+GB|/\d+GB)`
fn is_memory_field(field: &str) -> bool {
    let alternative = |middle: &str, suffix: &str| {
        field
            .strip_suffix(suffix)
            .and_then(|head| head.split_once(middle))
            .is_some_and(|(used, total)| is_uint(used) && is_uint(total))
    };
    alternative("/", "MB") || alternative("MB/", "GB") || alternative("/", "GB")
}

/// Whether `line` matches the Phase 01 verification regex
/// `^\d+(/\d+MB|MB/\d+GB|/\d+GB) \[[| ]{10}\] +\d+\.\d%( \d+\.\d\d){3}$`.
///
/// The CPU field is also allowed to be the six-wide integer form `cpu_text`
/// documents for values at or above 100%, so a runner whose CPU happens to be
/// pegged for the whole sampling window does not fail on a formatting rule the
/// binary is following correctly.
fn matches_one_line_format(line: &str) -> bool {
    let Some((memory, rest)) = line.split_once(" [") else {
        return false;
    };
    let Some((bar, rest)) = rest.split_once(']') else {
        return false;
    };
    if !is_memory_field(memory) {
        return false;
    }
    if bar.chars().count() != 10 || !bar.chars().all(|cell| cell == '|' || cell == ' ') {
        return false;
    }

    let mut fields = rest.split_whitespace();
    let Some(cpu) = fields.next().and_then(|field| field.strip_suffix('%')) else {
        return false;
    };
    if !is_fixed(cpu, 1) && !is_uint(cpu) {
        return false;
    }
    let averages: Vec<&str> = fields.collect();
    averages.len() == 3 && averages.iter().all(|value| is_fixed(value, 2))
}

// ---------------------------------------------------------------------------
// One-line mode
// ---------------------------------------------------------------------------

#[test]
fn the_default_invocation_prints_the_original_one_line_format() {
    let dir = TempDir::new("default-line");
    let line = one_line(&dir, &["--interval", "1"]);
    assert!(
        matches_one_line_format(&line),
        "{line:?} does not match the one-line format"
    );
}

#[test]
fn the_blocks_style_frames_the_bar() {
    let dir = TempDir::new("blocks");
    let line = one_line(&dir, &["--interval", "1", "--graph-style", "blocks"]);
    assert!(line.contains(FRAME_LEFT), "{line:?}");
    assert!(line.contains(FRAME_RIGHT), "{line:?}");
}

#[test]
fn the_vertical_style_draws_one_glyph_between_the_frames() {
    let dir = TempDir::new("vertical");
    let line = one_line(&dir, &["--interval", "1", "-v"]);

    let (_, rest) = line
        .split_once(FRAME_LEFT)
        .unwrap_or_else(|| panic!("{line:?} has no left frame"));
    let (inside, _) = rest
        .split_once(FRAME_RIGHT)
        .unwrap_or_else(|| panic!("{line:?} has no right frame"));

    assert_eq!(inside.chars().count(), 1, "{line:?} drew {inside:?}");
    assert!(
        inside.chars().all(|glyph| VERTICAL_GLYPHS.contains(glyph)),
        "{inside:?} is not a vertical graph glyph"
    );
}

#[test]
fn memory_mode_two_prints_a_percentage() {
    let dir = TempDir::new("mem-percent");
    let line = one_line(&dir, &["--interval", "1", "-m", "2"]);
    let percent = line
        .split_whitespace()
        .next()
        .and_then(|field| field.strip_suffix('%'))
        .unwrap_or_else(|| panic!("{line:?} does not start with a percentage"));
    assert!(is_fixed(percent, 2), "{percent:?}");
}

#[test]
fn zero_averages_prints_no_load_averages() {
    let dir = TempDir::new("no-averages");
    let line = one_line(&dir, &["--interval", "1", "-a", "0"]);
    assert!(line.ends_with('%'), "{line:?} still carries load averages");
    // The memory and CPU segments are untouched.
    assert!(line.contains('['), "{line:?}");
}

#[test]
fn threads_mode_may_exceed_one_hundred_percent() {
    let dir = TempDir::new("threads-mode");
    let line = one_line(&dir, &["--interval", "1", "-t", "1"]);
    // Whether it goes over 100 depends on how busy the machine is, so only the
    // shape is asserted: the field after the bar still parses as a number.
    let (_, rest) = line
        .split_once("] ")
        .unwrap_or_else(|| panic!("{line:?} has no CPU bar"));
    let percent = rest
        .split_whitespace()
        .next()
        .and_then(|field| field.strip_suffix('%'))
        .unwrap_or_else(|| panic!("{line:?} has no CPU percentage"));
    let value: f64 = percent
        .parse()
        .unwrap_or_else(|error| panic!("{percent:?} is not a number: {error}"));
    assert!(value >= 0.0, "{value}");
}

#[test]
fn invalid_arguments_fail_with_a_message_and_no_output() {
    let dir = TempDir::new("invalid-arguments");
    for args in [
        vec!["-a", "5"],
        vec!["-m", "9"],
        vec!["--graph-style", "nope"],
        vec!["-i", "0"],
    ] {
        let output = run(&dir, &args);
        assert!(!output.status.success(), "{args:?} was accepted");
        assert!(output.stdout.is_empty(), "{args:?} printed on stdout");
        assert!(!stderr_of(&output).is_empty(), "{args:?} said nothing");
    }
}

#[test]
fn print_config_round_trips_through_the_file_format() {
    let dir = TempDir::new("print-config");
    let output = run(&dir, &["--print-config", "--interval", "4", "-g", "12"]);
    assert!(output.status.success(), "{}", stderr_of(&output));

    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    let config: FileConfig =
        toml::from_str(&printed).unwrap_or_else(|error| panic!("{printed}\n{error}"));

    assert_eq!(config.interval_secs, Some(4));
    assert_eq!(config.graph_lines, Some(12));
    // Every key is spelled out, so the output is a complete config file.
    assert!(config.thresholds.is_some());
    assert_eq!(config.source.as_deref(), Some("system-monitor"));
}

// ---------------------------------------------------------------------------
// Daemon mode, against a fake herdr
// ---------------------------------------------------------------------------

/// Write a stand-in for the `herdr` CLI that appends its argv to `log`, one
/// invocation per line, answers `workspace list` with [`WORKSPACE_LIST`], and
/// exits with `code`.
#[cfg(unix)]
fn write_fake_herdr(dir: &TempDir, log: &Path, code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("fake-herdr");
    let script = format!(
        "#!/bin/sh\n\
         for arg in \"$@\"; do\n\
         \tprintf '%s ' \"$arg\" >> \"{log}\"\n\
         done\n\
         printf '\\n' >> \"{log}\"\n\
         if [ \"$1\" = workspace ] && [ \"$2\" = list ]; then\n\
         \tprintf '%s\\n' '{json}'\n\
         fi\n\
         exit {code}\n",
        log = log.display(),
        json = WORKSPACE_LIST,
    );
    std::fs::write(&path, script).expect("the fake herdr is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the fake herdr is executable");
    path
}

/// The same fake, in the only language a Windows runner executes without a
/// shebang.
#[cfg(windows)]
fn write_fake_herdr(dir: &TempDir, log: &Path, code: i32) -> PathBuf {
    let path = dir.join("fake-herdr.cmd");
    let script = format!(
        "@echo off\r\n\
         >>\"{log}\" echo %*\r\n\
         if \"%1\"==\"workspace\" if \"%2\"==\"list\" echo {json}\r\n\
         exit /b {code}\r\n",
        log = log.display(),
        json = WORKSPACE_LIST,
    );
    std::fs::write(&path, script).expect("the fake herdr is written");
    path
}

fn spawn_daemon(dir: &TempDir, fake: &Path) -> Child {
    spawn_daemon_with(dir, fake, &[])
}

fn spawn_daemon_with(dir: &TempDir, fake: &Path, extra: &[&str]) -> Child {
    command(dir)
        .args(["--daemon", "--interval", "1"])
        .args(extra)
        .env("HERDR_BIN_PATH", fake)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the daemon starts")
}

/// Every recorded invocation, one per line.
fn recorded(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn the_daemon_reports_tokens_for_every_workspace() {
    let dir = TempDir::new("daemon-reports");
    let log = dir.join("argv.log");
    let fake = write_fake_herdr(&dir, &log, 0);

    let mut daemon = spawn_daemon(&dir, &fake);
    std::thread::sleep(Duration::from_secs(3));
    daemon.kill().expect("the daemon is killed");
    daemon.wait().expect("the daemon is reaped");

    let calls = recorded(&log);
    assert!(
        calls.iter().any(|call| call.contains("workspace list")),
        "the daemon never listed the workspaces: {calls:?}"
    );

    for workspace in ["w1", "w9"] {
        let call = calls
            .iter()
            .find(|call| {
                call.contains("report-metadata") && call.contains(&format!(" {workspace} "))
            })
            .unwrap_or_else(|| panic!("no report for {workspace} in {calls:?}"));

        assert!(call.contains("--source system-monitor"), "{call}");
        assert!(call.contains("--ttl-ms"), "{call}");
        assert!(call.contains("--seq"), "{call}");
        for token in [
            "cpu_status=",
            "mem_status=",
            "load_status=",
            "sys_status=",
            "cpu_history=",
        ] {
            assert!(call.contains(token), "{token} missing from {call}");
        }
        // The level tokens that do not apply are cleared rather than set.
        assert!(call.contains("--clear-token"), "{call}");
    }
}

#[test]
fn the_focused_scope_reports_to_the_active_workspace_alone() {
    let dir = TempDir::new("daemon-focused");
    let log = dir.join("argv.log");
    let fake = write_fake_herdr(&dir, &log, 0);

    let mut daemon = spawn_daemon_with(&dir, &fake, &["--workspaces", "focused"]);
    std::thread::sleep(Duration::from_secs(3));
    daemon.kill().expect("the daemon is killed");
    daemon.wait().expect("the daemon is reaped");

    let calls = recorded(&log);
    let reports: Vec<&String> = calls
        .iter()
        .filter(|call| call.contains("report-metadata"))
        .collect();
    assert!(
        !reports.is_empty(),
        "the daemon reported nothing: {calls:?}"
    );

    // w9 is the focused workspace in WORKSPACE_LIST; w1 never hears from the
    // daemon at all.
    for report in &reports {
        assert!(report.contains(" w9 "), "{report}");
        assert!(
            !report.contains(" w1 "),
            "an unfocused workspace was reported to: {report}"
        );
    }
    assert!(
        reports.iter().any(|report| report.contains("cpu_status=")),
        "the focused workspace got no tokens: {reports:?}"
    );
}

/// One `--set-token`'s value: everything after `<key>=` up to the next flag.
///
/// A token value never contains ` --`, so the next flag is where it ends. The
/// fake herdr records one invocation per line with the arguments run together,
/// which is the only reason this has to be picked apart at all.
fn token_value(call: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let start = call.find(&marker)? + marker.len();
    let rest = &call[start..];
    let end = rest.find(" --").unwrap_or(rest.len());
    Some(rest[..end].trim_end().to_string())
}

/// The number of cells in the framed bar a token value opens with.
///
/// The closing frame character doubles as the one-eighth block, so the bar
/// ends at the *last* one in the value rather than the first; the reading that
/// follows the bar never contains it.
fn bar_cells(value: &str) -> Option<usize> {
    let inner = value.strip_prefix(FRAME_LEFT)?;
    let end = inner.rfind(FRAME_RIGHT)?;
    Some(inner[..end].chars().count())
}

/// Wait for a report whose CPU bar is `cells` wide, and return whether one
/// arrived inside `budget`.
fn wait_for_cpu_bar(log: &Path, cells: usize, budget: Duration) -> bool {
    let started = Instant::now();
    loop {
        let seen = recorded(log).iter().any(|call| {
            token_value(call, "cpu_status")
                .and_then(|value| bar_cells(&value))
                .is_some_and(|width| width == cells)
        });
        if seen {
            return true;
        }
        if started.elapsed() > budget {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_daemon_picks_up_a_configuration_edit_without_restarting() {
    let dir = TempDir::new("daemon-reload");
    let log = dir.join("argv.log");
    let fake = write_fake_herdr(&dir, &log, 0);
    // `command` points HERDR_PLUGIN_CONFIG_DIR at this directory, so this is
    // the file the daemon resolves its settings from.
    let config = dir.join("config.toml");
    std::fs::write(&config, "graph_lines = 2\n").expect("the config is written");

    let mut daemon = spawn_daemon(&dir, &fake);
    // The first tick only establishes the CPU baseline, so the first report
    // lands on the second one.
    let budget = Duration::from_secs(15);
    let narrow = wait_for_cpu_bar(&log, 2, budget);
    if !narrow {
        daemon.kill().ok();
        daemon.wait().ok();
        panic!(
            "no report with the configured 2 cell bar: {:?}",
            recorded(&log)
        );
    }

    // The edit a running daemon used to need a restart for. A different length
    // as well as a different mtime, so it is noticed whatever the filesystem's
    // timestamp granularity.
    std::fs::write(&config, "graph_lines = 20\n").expect("the config is rewritten");
    let widened = wait_for_cpu_bar(&log, 20, budget);

    daemon.kill().expect("the daemon is killed");
    daemon.wait().expect("the daemon is reaped");
    assert!(
        widened,
        "the daemon never picked up the edit: {:?}",
        recorded(&log)
    );
}

#[test]
fn the_daemon_gives_up_once_herdr_stops_answering() {
    let dir = TempDir::new("daemon-server-gone");
    let log = dir.join("argv.log");
    let fake = write_fake_herdr(&dir, &log, 1);

    let mut daemon = spawn_daemon(&dir, &fake);
    // The first tick only establishes the CPU baseline, so the failures start
    // on the second one.
    let budget = Duration::from_secs(u64::from(DEFAULT_MAX_FAILURES) + 2);
    let started = Instant::now();

    let status = loop {
        match daemon.try_wait().expect("the daemon can be polled") {
            Some(status) => break status,
            None if started.elapsed() > budget => {
                daemon.kill().ok();
                daemon.wait().ok();
                panic!("the daemon was still running after {budget:?}");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };

    assert!(
        status.success(),
        "a herdr that stopped answering is an ordinary shutdown, got {status:?}"
    );
    assert!(
        !recorded(&log).is_empty(),
        "the daemon never tried to reach herdr"
    );
}
