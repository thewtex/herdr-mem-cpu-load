//! The configuration file layer.
//!
//! herdr hands every plugin a private config directory in
//! `HERDR_PLUGIN_CONFIG_DIR`. A `config.toml` there sets anything the command
//! line can set, so the sidebar can be tuned without editing the manifest's
//! startup command and re-linking the plugin.
//!
//! Four layers are merged, highest first:
//!
//! 1. explicit command line flags,
//! 2. the file named by `--config <PATH>`,
//! 3. `$HERDR_PLUGIN_CONFIG_DIR/config.toml`,
//! 4. the built-in defaults.
//!
//! Every [`FileConfig`] field is an `Option`, so a file may set one key and
//! leave the rest alone, and [`resolve`] produces the fully populated
//! [`Settings`] that daemon mode and one-line mode both consume.

use std::fmt;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cli::Cli;
use crate::daemon::{default_ttl_ms, DaemonOptions, DEFAULT_MAX_FAILURES};
use crate::metrics::cpu::sampling_delay;
use crate::metrics::memory::MemoryMode;
use crate::metrics::CpuMode;
use crate::render::colors::{ColorMode, PowerlineMode};
use crate::render::format::RenderOptions;
use crate::render::graph::GraphStyle;
use crate::tokens::{Thresholds, TokenOptions};

/// The file `HERDR_PLUGIN_CONFIG_DIR` is searched for.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// The environment variable herdr sets to the plugin's config directory.
pub const CONFIG_DIR_ENV: &str = "HERDR_PLUGIN_CONFIG_DIR";
/// The commented template `--write-default-config` writes out.
pub const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("config/default_config.toml");

/// The default sampling interval, in seconds.
pub const DEFAULT_INTERVAL_SECS: u64 = 1;
/// The longest sampling interval. An hour between samples is already past the
/// point where a "live" status line means anything, and the CPU measurement
/// window is the interval, so one-line mode would block for that long.
pub const MAX_INTERVAL_SECS: u64 = 3600;
/// The shortest token lifetime herdr accepts.
pub const MIN_TTL_MS: u64 = 1;
/// The longest token lifetime herdr accepts: 24 hours.
pub const MAX_TTL_MS: u64 = 86_400_000;
/// The default width of the CPU graph, in cells.
pub const DEFAULT_GRAPH_LINES: usize = 10;
/// The widest bar the command line accepts.
///
/// A token value is capped at [`crate::tokens::MAX_TOKEN_VALUE_CHARS`]
/// characters, and past 64 cells there is no room left for the reading beside
/// the bar. [`crate::tokens::TokenOptions`] narrows a bar further still when
/// the reading is long; this is only the point where asking is a mistake
/// rather than a preference.
pub const MAX_GRAPH_LINES: usize = 64;
/// The default number of load averages printed.
pub const DEFAULT_AVERAGES_COUNT: u8 = 3;
/// The default metadata source id.
pub const DEFAULT_SOURCE: &str = "system-monitor";

/// Everything a `config.toml` may set. Every field is optional so a file can
/// override one key and inherit the rest.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Seconds between samples; also the CPU measurement window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    /// How long herdr keeps the reported tokens, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    /// The metadata source the tokens are reported under.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// `classic`, `blocks`, or `vertical`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_style: Option<GraphStyle>,
    /// Cells in the CPU graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_lines: Option<usize>,
    /// Cells in the memory bar. Defaults to `graph_lines`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_graph_lines: Option<usize>,
    /// 0: used/total, 1: free memory, 2: usage percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_mode: Option<MemoryMode>,
    /// 0: max 100%, 1: max 100% per thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_mode: Option<CpuMode>,
    /// How many load averages to print, 0 to 3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub averages_count: Option<u8>,
    /// Samples kept for the `$cpu_history` sparkline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<usize>,
    /// Log every tick's status line, not just errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    /// Where each metric stops being comfortable. Must come last: TOML puts
    /// tables after the scalars that share their parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thresholds: Option<FileThresholds>,
}

/// The `[thresholds]` table of a `config.toml`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileThresholds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_warn: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_hot: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_warn: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_hot: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_warn: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_hot: Option<f64>,
}

impl FileThresholds {
    /// Fill the unset keys in from `defaults`.
    #[must_use]
    pub fn merge(self, defaults: Thresholds) -> Thresholds {
        Thresholds {
            cpu_warn: self.cpu_warn.unwrap_or(defaults.cpu_warn),
            cpu_hot: self.cpu_hot.unwrap_or(defaults.cpu_hot),
            mem_warn: self.mem_warn.unwrap_or(defaults.mem_warn),
            mem_hot: self.mem_hot.unwrap_or(defaults.mem_hot),
            load_warn: self.load_warn.unwrap_or(defaults.load_warn),
            load_hot: self.load_hot.unwrap_or(defaults.load_hot),
        }
    }

    /// Every key spelled out, for `--print-config`.
    #[must_use]
    pub const fn from_thresholds(thresholds: Thresholds) -> Self {
        Self {
            cpu_warn: Some(thresholds.cpu_warn),
            cpu_hot: Some(thresholds.cpu_hot),
            mem_warn: Some(thresholds.mem_warn),
            mem_hot: Some(thresholds.mem_hot),
            load_warn: Some(thresholds.load_warn),
            load_hot: Some(thresholds.load_hot),
        }
    }
}

/// A config file that could not be read or understood.
///
/// The path is carried alongside the message because a `toml` error already
/// names the line and column but has no idea which file it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ConfigError {}

/// Parse a `config.toml`.
///
/// A missing file is `Ok(None)`: herdr creates the config directory for every
/// plugin whether or not the plugin wants one, so an absent file is the normal
/// case rather than a problem.
///
/// # Errors
///
/// Returns a [`ConfigError`] when the file exists but cannot be read, is not
/// valid TOML, or mentions a key [`FileConfig`] does not have.
pub fn load(path: &Path) -> Result<Option<FileConfig>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ConfigError {
                path: path.to_path_buf(),
                message: error.to_string(),
            })
        }
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|error| ConfigError {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

/// The `config.toml` this run should read, if any.
///
/// `--config` wins; otherwise the plugin's own config directory is searched.
#[must_use]
pub fn config_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    config_dir().map(|dir| dir.join(CONFIG_FILE_NAME))
}

/// The plugin config directory herdr injected, if it injected one.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os(CONFIG_DIR_ENV)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// Read the configuration file for this run, reporting a bad one on stderr
/// rather than failing.
///
/// A typo in `config.toml` must not take the sidebar down with it, so a
/// malformed file is announced and then ignored.
#[must_use]
pub fn load_or_warn(explicit: Option<&Path>) -> Option<FileConfig> {
    let path = config_path(explicit)?;
    match load(&path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("herdr-mem-cpu-load: ignoring {error}");
            None
        }
    }
}

/// Every option, fully resolved: what [`crate::daemon::run`], one-line mode,
/// and `--watch` all read.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub interval_secs: u64,
    pub ttl_ms: u64,
    pub source: String,
    pub graph_style: GraphStyle,
    pub graph_lines: usize,
    pub mem_graph_lines: usize,
    pub mem_mode: MemoryMode,
    pub cpu_mode: CpuMode,
    pub averages_count: u8,
    pub history_len: usize,
    pub verbose: bool,
    pub thresholds: Thresholds,
    /// Where daemon diagnostics are appended. Command line only.
    pub log: Option<PathBuf>,
    /// Which colour markup one-line and watch mode emit. Command line only.
    pub color: ColorMode,
    /// Which powerline separators the tmux markup blends with.
    pub powerline: PowerlineMode,
    pub segments_left: Option<u16>,
    pub segments_right: Option<u16>,
}

impl Default for Settings {
    fn default() -> Self {
        let interval = Duration::from_secs(DEFAULT_INTERVAL_SECS);
        Self {
            interval_secs: DEFAULT_INTERVAL_SECS,
            ttl_ms: default_ttl_ms(interval),
            source: DEFAULT_SOURCE.to_string(),
            graph_style: GraphStyle::Classic,
            graph_lines: DEFAULT_GRAPH_LINES,
            mem_graph_lines: DEFAULT_GRAPH_LINES,
            mem_mode: MemoryMode::Default,
            cpu_mode: CpuMode::Default,
            averages_count: DEFAULT_AVERAGES_COUNT,
            history_len: DEFAULT_GRAPH_LINES,
            verbose: false,
            thresholds: Thresholds::default(),
            log: None,
            color: ColorMode::None,
            powerline: PowerlineMode::None,
            segments_left: None,
            segments_right: None,
        }
    }
}

impl Settings {
    /// The sampling interval.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs)
    }

    /// How long one-line mode measures the CPU before printing.
    #[must_use]
    pub fn sampling_delay(&self) -> Duration {
        sampling_delay(self.interval_secs)
    }

    /// The one-line renderer's options.
    #[must_use]
    pub const fn render_options(&self) -> RenderOptions {
        RenderOptions {
            mem_mode: self.mem_mode,
            cpu_mode: self.cpu_mode,
            graph_style: self.graph_style,
            graph_lines: self.graph_lines,
            averages_count: self.averages_count,
            color: self.color,
            powerline: self.powerline,
            segments_left: self.segments_left,
            segments_right: self.segments_right,
        }
    }

    /// The sidebar token builder's options.
    #[must_use]
    pub const fn token_options(&self) -> TokenOptions {
        TokenOptions {
            graph_style: self.graph_style,
            graph_lines: self.graph_lines,
            mem_graph_lines: self.mem_graph_lines,
            mem_mode: self.mem_mode,
            cpu_mode: self.cpu_mode,
            averages_count: self.averages_count,
            thresholds: self.thresholds,
        }
    }

    /// Everything [`crate::daemon::run`] needs.
    #[must_use]
    pub fn daemon_options(&self) -> DaemonOptions {
        DaemonOptions {
            interval: self.interval(),
            ttl_ms: self.ttl_ms,
            source: self.source.clone(),
            tokens: self.token_options(),
            history_len: self.history_len,
            max_failures: DEFAULT_MAX_FAILURES,
            log: self.log.clone(),
            verbose: self.verbose,
        }
    }

    /// These settings written back out in `config.toml` shape, which is what
    /// `--print-config` prints.
    #[must_use]
    pub fn to_file_config(&self) -> FileConfig {
        FileConfig {
            interval_secs: Some(self.interval_secs),
            ttl_ms: Some(self.ttl_ms),
            source: Some(self.source.clone()),
            graph_style: Some(self.graph_style),
            graph_lines: Some(self.graph_lines),
            mem_graph_lines: Some(self.mem_graph_lines),
            mem_mode: Some(self.mem_mode),
            cpu_mode: Some(self.cpu_mode),
            averages_count: Some(self.averages_count),
            history: Some(self.history_len),
            verbose: Some(self.verbose),
            thresholds: Some(FileThresholds::from_thresholds(self.thresholds)),
        }
    }

    /// `--print-config` output: the effective settings as a `config.toml`.
    ///
    /// # Errors
    ///
    /// Returns the serializer's message, which in practice cannot happen for a
    /// struct of scalars.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(&self.to_file_config())
    }
}

/// Merge the command line over the configuration file over the defaults.
#[must_use]
pub fn resolve(cli: &Cli, file: Option<FileConfig>) -> Settings {
    let file = file.unwrap_or_default();
    let defaults = Settings::default();

    // The command line is range checked by clap; a configuration file is not,
    // and neither is the ttl derived from an interval, so both are clamped
    // here as well.
    let interval_secs = cli
        .interval
        .or(file.interval_secs)
        .unwrap_or(defaults.interval_secs)
        .clamp(1, MAX_INTERVAL_SECS);
    let interval = Duration::from_secs(interval_secs);

    let graph_lines = cli
        .graph_lines
        .or(file.graph_lines)
        .unwrap_or(defaults.graph_lines)
        .min(MAX_GRAPH_LINES);
    let mem_graph_lines = cli
        .mem_graph_lines
        .or(file.mem_graph_lines)
        .unwrap_or(graph_lines)
        .min(MAX_GRAPH_LINES);

    // The two modes want different bars: the ASCII one on a tmux status line,
    // unicode blocks in the herdr sidebar.
    let fallback_style = if cli.daemon.enabled {
        GraphStyle::Blocks
    } else {
        GraphStyle::Classic
    };
    let graph_style = if cli.vertical_graph {
        GraphStyle::Vertical
    } else {
        cli.graph_style
            .or(file.graph_style)
            .unwrap_or(fallback_style)
    };

    let powerline = if cli.compat.powerline_right {
        PowerlineMode::Right
    } else if cli.compat.powerline_left {
        PowerlineMode::Left
    } else {
        PowerlineMode::None
    };
    let color = resolve_color(cli, powerline);

    Settings {
        interval_secs,
        ttl_ms: cli
            .daemon
            .ttl_ms
            .or(file.ttl_ms)
            .unwrap_or_else(|| default_ttl_ms(interval))
            .clamp(MIN_TTL_MS, MAX_TTL_MS),
        source: cli
            .daemon
            .source
            .clone()
            .or(file.source)
            .unwrap_or(defaults.source),
        graph_style,
        graph_lines,
        mem_graph_lines,
        mem_mode: cli.mem_mode.or(file.mem_mode).unwrap_or(defaults.mem_mode),
        cpu_mode: cli.cpu_mode.or(file.cpu_mode).unwrap_or(defaults.cpu_mode),
        averages_count: cli
            .averages_count
            .or(file.averages_count)
            .unwrap_or(defaults.averages_count)
            .min(3),
        history_len: cli.daemon.history.or(file.history).unwrap_or(graph_lines),
        verbose: cli.daemon.verbose || file.verbose.unwrap_or(false),
        thresholds: file
            .thresholds
            .unwrap_or_default()
            .merge(defaults.thresholds),
        log: cli.daemon.log_file.clone(),
        color,
        powerline,
        segments_left: cli.compat.segments_left,
        segments_right: cli.compat.segments_right,
    }
}

/// Which colour markup this invocation emits.
///
/// `--ansi` wins, then `-c` and the powerline flags, which imply it the way
/// the original does. Failing all of those, `--watch` colours itself when it
/// is talking to a terminal and stays plain when its output is redirected.
fn resolve_color(cli: &Cli, powerline: PowerlineMode) -> ColorMode {
    if cli.ansi {
        ColorMode::Ansi
    } else if cli.compat.colors || powerline != PowerlineMode::None {
        ColorMode::Tmux
    } else if cli.watch && std::io::stdout().is_terminal() {
        ColorMode::Ansi
    } else {
        ColorMode::None
    }
}

/// Where `--write-default-config` writes.
///
/// # Errors
///
/// Returns a message when neither `--config` nor `HERDR_PLUGIN_CONFIG_DIR`
/// says where the file should go.
pub fn default_config_target(explicit: Option<&Path>) -> Result<PathBuf, String> {
    config_path(explicit)
        .ok_or_else(|| format!("no config directory: pass --config <PATH> or set {CONFIG_DIR_ENV}"))
}

/// Write the commented template to `path`.
///
/// # Errors
///
/// Returns a [`ConfigError`] when the file already exists and `force` is not
/// set, or when the directory or file cannot be written.
pub fn write_default_config(path: &Path, force: bool) -> Result<(), ConfigError> {
    let fail = |message: String| ConfigError {
        path: path.to_path_buf(),
        message,
    };
    if !force && path.exists() {
        return Err(fail(
            "already exists; pass --force to overwrite".to_string(),
        ));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| fail(error.to_string()))?;
        }
    }
    std::fs::write(path, DEFAULT_CONFIG_TEMPLATE).map_err(|error| fail(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        load, resolve, write_default_config, FileConfig, Settings, DEFAULT_CONFIG_TEMPLATE,
        MAX_GRAPH_LINES, MAX_INTERVAL_SECS, MAX_TTL_MS, MIN_TTL_MS,
    };
    use crate::cli::Cli;
    use crate::metrics::memory::MemoryMode;
    use crate::metrics::CpuMode;
    use crate::render::graph::GraphStyle;
    use clap::Parser;
    use std::path::PathBuf;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("herdr-mem-cpu-load").chain(args.iter().copied()))
            .expect("arguments parse")
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "herdr-mem-cpu-load-test-{}-{name}",
            std::process::id()
        ));
        path
    }

    #[test]
    fn defaults_apply_when_nothing_is_configured() {
        let settings = resolve(&parse(&[]), None);
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.graph_style, GraphStyle::Classic);
        assert_eq!(settings.ttl_ms, 3000);
    }

    #[test]
    fn the_daemon_defaults_to_the_sidebar_bar() {
        assert_eq!(
            resolve(&parse(&["--daemon"]), None).graph_style,
            GraphStyle::Blocks
        );
    }

    #[test]
    fn a_file_overrides_the_defaults() {
        let file: FileConfig = toml::from_str(
            r#"
            interval_secs = 5
            graph_lines = 20
            mem_mode = 2
            cpu_mode = 1
            averages_count = 1
            source = "box"
            verbose = true
            graph_style = "vertical"

            [thresholds]
            cpu_warn = 5.5
            load_hot = 4.0
            "#,
        )
        .expect("the file parses");

        let settings = resolve(&parse(&[]), Some(file));
        assert_eq!(settings.interval_secs, 5);
        assert_eq!(settings.graph_lines, 20);
        // mem_graph_lines follows graph_lines when it is not set itself.
        assert_eq!(settings.mem_graph_lines, 20);
        assert_eq!(settings.history_len, 20);
        assert_eq!(settings.mem_mode, MemoryMode::UsagePercent);
        assert_eq!(settings.cpu_mode, CpuMode::Threads);
        assert_eq!(settings.averages_count, 1);
        assert_eq!(settings.source, "box");
        assert!(settings.verbose);
        assert_eq!(settings.graph_style, GraphStyle::Vertical);
        // The ttl still follows the interval it was not given alongside.
        assert_eq!(settings.ttl_ms, 11_000);
        // Unset threshold keys keep their defaults.
        assert!((settings.thresholds.cpu_warn - 5.5).abs() < f32::EPSILON);
        assert!((settings.thresholds.cpu_hot - 80.0).abs() < f32::EPSILON);
        assert!((settings.thresholds.load_hot - 4.0).abs() < f64::EPSILON);
        assert!((settings.thresholds.load_warn - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn the_command_line_wins_over_the_file() {
        let file: FileConfig =
            toml::from_str("interval_secs = 5\ngraph_lines = 20\n").expect("the file parses");
        let settings = resolve(&parse(&["--interval", "3", "-g", "4"]), Some(file));
        assert_eq!(settings.interval_secs, 3);
        assert_eq!(settings.graph_lines, 4);
        assert_eq!(settings.mem_graph_lines, 4);
    }

    #[test]
    fn the_memory_bar_can_be_set_on_its_own() {
        let settings = resolve(&parse(&["-g", "12", "--mem-graph-lines", "4"]), None);
        assert_eq!(settings.graph_lines, 12);
        assert_eq!(settings.mem_graph_lines, 4);
        assert_eq!(settings.token_options().mem_graph_lines, 4);
    }

    #[test]
    fn a_file_cannot_escape_the_ranges_the_command_line_enforces() {
        // clap range checks the command line; nothing range checks a file, so
        // `resolve` has to.
        let high: FileConfig =
            toml::from_str("interval_secs = 100000\nttl_ms = 999999999\ngraph_lines = 400\n")
                .expect("the file parses");
        let settings = resolve(&parse(&[]), Some(high));
        assert_eq!(settings.interval_secs, MAX_INTERVAL_SECS);
        assert_eq!(settings.ttl_ms, MAX_TTL_MS);
        assert_eq!(settings.graph_lines, MAX_GRAPH_LINES);
        assert_eq!(settings.mem_graph_lines, MAX_GRAPH_LINES);

        let low: FileConfig =
            toml::from_str("interval_secs = 0\nttl_ms = 0\n").expect("the file parses");
        let settings = resolve(&parse(&[]), Some(low));
        assert_eq!(settings.interval_secs, 1);
        assert_eq!(settings.ttl_ms, MIN_TTL_MS);
    }

    #[test]
    fn a_long_interval_does_not_push_the_default_ttl_out_of_range() {
        let settings = resolve(&parse(&["--interval", "3600"]), None);
        assert_eq!(settings.interval_secs, MAX_INTERVAL_SECS);
        assert_eq!(settings.ttl_ms, 7_201_000);
        assert!(settings.ttl_ms <= MAX_TTL_MS);
    }

    #[test]
    fn an_unknown_key_names_the_file_and_the_line() {
        let path = temp_path("unknown-key.toml");
        std::fs::write(&path, "interval_secs = 2\ngrpah_lines = 10\n").expect("write");
        let error = load(&path).expect_err("an unknown key is rejected");
        std::fs::remove_file(&path).ok();

        assert_eq!(error.path, path);
        assert!(error.message.contains("grpah_lines"), "{}", error.message);
        assert!(error.to_string().contains("unknown-key.toml"));
    }

    #[test]
    fn an_out_of_range_mode_is_a_file_error() {
        let path = temp_path("bad-mode.toml");
        std::fs::write(&path, "mem_mode = 7\n").expect("write");
        let error = load(&path).expect_err("an invalid mode is rejected");
        std::fs::remove_file(&path).ok();
        assert!(
            error.message.contains("invalid memory mode"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        assert_eq!(load(&temp_path("definitely-absent.toml")), Ok(None));
    }

    #[test]
    fn the_template_parses_back_into_an_empty_config() {
        let config: FileConfig =
            toml::from_str(DEFAULT_CONFIG_TEMPLATE).expect("the template is valid TOML");
        assert_eq!(config, FileConfig::default());
    }

    #[test]
    fn print_config_round_trips_through_the_file_format() {
        let settings = resolve(&parse(&["--interval", "4", "--source", "box"]), None);
        let printed = settings.to_toml().expect("settings serialise");

        let parsed: FileConfig = toml::from_str(&printed).expect("the output parses back");
        assert_eq!(parsed, settings.to_file_config());
        assert_eq!(resolve(&parse(&[]), Some(parsed)), settings);
    }

    #[test]
    fn the_default_config_is_not_written_over_an_existing_file() {
        let path = temp_path("write-default.toml");
        std::fs::remove_file(&path).ok();

        write_default_config(&path, false).expect("the first write succeeds");
        let error = write_default_config(&path, false).expect_err("the second refuses");
        assert!(error.message.contains("--force"), "{}", error.message);
        write_default_config(&path, true).expect("--force overwrites");

        let written = std::fs::read_to_string(&path).expect("read back");
        std::fs::remove_file(&path).ok();
        assert_eq!(written, DEFAULT_CONFIG_TEMPLATE);
    }
}
