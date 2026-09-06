//! Command line interface, kept flag-compatible with `tmux-mem-cpu-load` so
//! existing tmux configurations keep working.
//!
//! Every option that a `config.toml` can also set is an `Option` here, so
//! [`crate::config::resolve`] can tell "the user asked for 10" apart from "the
//! user said nothing and 10 is the default".

use std::path::{Path, PathBuf};

use clap::{Args, Parser};

use crate::config::{self, Settings, MAX_GRAPH_LINES, MAX_INTERVAL_SECS, MAX_TTL_MS, MIN_TTL_MS};
use crate::daemon::WorkspaceScope;
use crate::metrics::memory::MemoryMode;
use crate::metrics::CpuMode;
use crate::render::graph::GraphStyle;
use crate::tokens::MAX_TOKEN_VALUE_CHARS;

/// CPU, memory, and load average monitor for herdr and tmux.
///
/// The `bool` count is what a command line looks like; clap needs one field
/// per switch and there is no state machine hiding in them.
#[derive(Debug, Parser)]
#[command(name = "herdr-mem-cpu-load", version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Status refresh interval in seconds; also the CPU sampling window.
    /// [default: 1]
    #[arg(
        short = 'i',
        long,
        value_name = "SECS",
        value_parser = clap::value_parser!(u64).range(1..=MAX_INTERVAL_SECS)
    )]
    pub interval: Option<u64>,

    /// How many cells the CPU graph is drawn with. 0 hides the graph.
    /// [default: 10]
    #[arg(short = 'g', long, value_name = "N", value_parser = parse_graph_lines)]
    pub graph_lines: Option<usize>,

    /// How many cells the memory bar is drawn with. [default: --graph-lines]
    #[arg(long, value_name = "N", value_parser = parse_graph_lines)]
    pub mem_graph_lines: Option<usize>,

    /// How many cells the load bar is drawn with. [default: --graph-lines
    /// when it is given, otherwise 4]
    #[arg(long, value_name = "N", value_parser = parse_graph_lines)]
    pub load_graph_lines: Option<usize>,

    /// Memory display mode. 0: used/total, 1: free memory, 2: usage percent.
    /// [default: 0]
    #[arg(short = 'm', long, value_name = "0|1|2", value_parser = parse_mem_mode)]
    pub mem_mode: Option<MemoryMode>,

    /// CPU display mode. 0: max 100%, 1: max 100% per thread. [default: 0]
    #[arg(short = 't', long, value_name = "0|1", value_parser = parse_cpu_mode)]
    pub cpu_mode: Option<CpuMode>,

    /// How many load averages to print. [default: 3]
    #[arg(
        short = 'a',
        long,
        value_name = "0-3",
        value_parser = clap::value_parser!(u8).range(0..=3)
    )]
    pub averages_count: Option<u8>,

    /// Use the single-character vertical bar chart for the CPU graph.
    #[arg(short = 'v', long)]
    pub vertical_graph: bool,

    /// CPU graph style: classic, blocks, or vertical. [default: classic;
    /// blocks with --daemon]
    #[arg(long, value_name = "STYLE")]
    pub graph_style: Option<GraphStyle>,

    /// Read this configuration file instead of the one in the herdr plugin
    /// config directory.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Print the effective configuration as TOML and exit.
    #[arg(long)]
    pub print_config: bool,

    /// Write a commented default config.toml and exit.
    #[arg(long)]
    pub write_default_config: bool,

    /// Let --write-default-config overwrite an existing file.
    #[arg(long)]
    pub force: bool,

    /// Add this plugin's rows to `[ui.sidebar.spaces]` in herdr's own
    /// config.toml, if no layout is set there, and exit.
    #[arg(long)]
    pub write_sidebar_rows: bool,

    /// Re-sample every interval and reprint the status line in place until
    /// interrupted.
    #[arg(long)]
    pub watch: bool,

    /// Colour the output with 256-colour ANSI escapes. Default in --watch when
    /// stdout is a terminal.
    #[arg(long)]
    pub ansi: bool,

    #[command(flatten)]
    pub daemon: DaemonFlags,

    #[command(flatten)]
    pub compat: CompatibilityFlags,
}

/// Flags that only mean something in daemon mode.
#[derive(Debug, Args)]
pub struct DaemonFlags {
    /// Sample on an interval and report Space sidebar tokens to herdr instead
    /// of printing one line.
    #[arg(long = "daemon")]
    pub enabled: bool,

    /// How long herdr keeps the reported tokens, in milliseconds.
    /// [default: twice the interval plus one second]
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u64).range(MIN_TTL_MS..=MAX_TTL_MS)
    )]
    pub ttl_ms: Option<u64>,

    /// The metadata source the tokens are reported under.
    /// [default: system-monitor]
    #[arg(long, value_name = "ID")]
    pub source: Option<String>,

    /// Which workspaces the tokens go to: focused for the active workspace
    /// alone, or all. [default: focused]
    #[arg(long, value_name = "all|focused")]
    pub workspaces: Option<WorkspaceScope>,

    /// How many samples the CPU history sparkline keeps.
    /// [default: --graph-lines when it is given, otherwise 20]
    #[arg(long, value_name = "N")]
    pub history: Option<usize>,

    /// Append daemon diagnostics to this file. Without it the daemon logs
    /// into the herdr plugin state directory, or nowhere at all.
    #[arg(long, value_name = "PATH")]
    pub log_file: Option<PathBuf>,

    /// Also log every tick's status line, not just errors.
    #[arg(long)]
    pub verbose: bool,
}

/// The colour and powerline flags of the original.
#[derive(Debug, Args)]
pub struct CompatibilityFlags {
    /// Emit tmux colour markup around each segment.
    #[arg(short = 'c', long)]
    pub colors: bool,

    /// Use left-pointing powerline separators. Implies --colors.
    #[arg(short = 'p', long)]
    pub powerline_left: bool,

    /// Use right-pointing powerline separators. Implies --colors.
    #[arg(short = 'q', long)]
    pub powerline_right: bool,

    /// Blend the first segment with this tmux colour, for a seamless join
    /// with whatever is drawn to the left of the status.
    #[arg(
        short = 'l',
        long,
        value_name = "COLOR",
        value_parser = clap::value_parser!(u16).range(0..=255)
    )]
    pub segments_left: Option<u16>,

    /// Blend the last segment with this tmux colour, for a seamless join with
    /// whatever is drawn to the right of the status.
    #[arg(
        short = 'r',
        long,
        value_name = "COLOR",
        value_parser = clap::value_parser!(u16).range(0..=255)
    )]
    pub segments_right: Option<u16>,
}

fn parse_mem_mode(value: &str) -> Result<MemoryMode, String> {
    parse_mode(value).and_then(|mode| MemoryMode::try_from(mode).map_err(|error| error.to_string()))
}

fn parse_cpu_mode(value: &str) -> Result<CpuMode, String> {
    parse_mode(value).and_then(|mode| CpuMode::try_from(mode).map_err(|error| error.to_string()))
}

/// A bar width, rejecting one so wide that the reading beside it could not
/// fit in a sidebar token.
fn parse_graph_lines(value: &str) -> Result<usize, String> {
    let cells: usize = value
        .parse()
        .map_err(|_| format!("`{value}` is not a number of cells"))?;
    if cells > MAX_GRAPH_LINES {
        return Err(format!(
            "{cells} cells is wider than the {MAX_GRAPH_LINES} cell maximum; \
             a sidebar token holds {MAX_TOKEN_VALUE_CHARS} characters, and a \
             wider bar leaves no room for the reading beside it"
        ));
    }
    Ok(cells)
}

fn parse_mode(value: &str) -> Result<u8, String> {
    value
        .parse::<u8>()
        .map_err(|_| format!("`{value}` is not a mode number"))
}

impl Cli {
    /// The configuration file this run reads, if any.
    #[must_use]
    pub fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    /// The command line merged over the configuration file over the defaults.
    ///
    /// A malformed configuration file is reported on stderr and then ignored,
    /// so a typo cannot stop the daemon from starting.
    #[must_use]
    pub fn settings(&self) -> Settings {
        config::resolve(self, config::load_or_warn(self.config_path()))
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use crate::daemon::WorkspaceScope;
    use crate::metrics::memory::MemoryMode;
    use crate::metrics::CpuMode;
    use crate::render::colors::{ColorMode, PowerlineMode};
    use crate::render::graph::GraphStyle;
    use clap::Parser;
    use std::time::Duration;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("herdr-mem-cpu-load").chain(args.iter().copied()))
            .expect("arguments parse")
    }

    /// Resolve without reading any file, so the test does not depend on
    /// whatever `HERDR_PLUGIN_CONFIG_DIR` happens to point at.
    fn settings(args: &[&str]) -> crate::config::Settings {
        crate::config::resolve(&parse(args), None)
    }

    #[test]
    fn defaults_match_the_original() {
        let opts = settings(&[]).render_options();
        assert_eq!(opts.mem_mode, MemoryMode::Default);
        assert_eq!(opts.cpu_mode, CpuMode::Default);
        assert_eq!(opts.graph_style, GraphStyle::Classic);
        assert_eq!(opts.graph_lines, 10);
        assert_eq!(opts.averages_count, 3);
        assert_eq!(opts.color, ColorMode::None);
        assert_eq!(settings(&[]).sampling_delay(), Duration::from_millis(990));
    }

    #[test]
    fn vertical_flag_overrides_the_graph_style() {
        assert_eq!(settings(&["-v"]).graph_style, GraphStyle::Vertical);
        assert_eq!(
            settings(&["--graph-style", "blocks"]).graph_style,
            GraphStyle::Blocks
        );
        // -v wins even when both are given, as in the original.
        assert_eq!(
            settings(&["-v", "--graph-style", "blocks"]).graph_style,
            GraphStyle::Vertical
        );
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        for args in [
            vec!["-a", "5"],
            vec!["-i", "0"],
            vec!["-m", "3"],
            vec!["-t", "2"],
            vec!["--graph-style", "spiral"],
            vec!["--workspaces", "everything"],
            vec!["-l", "256"],
            vec!["--ttl-ms", "0"],
        ] {
            assert!(
                Cli::try_parse_from(
                    std::iter::once("herdr-mem-cpu-load").chain(args.iter().copied())
                )
                .is_err(),
                "expected {args:?} to be rejected"
            );
        }
    }

    #[test]
    fn a_bar_wider_than_a_token_is_rejected_with_an_explanation() {
        let error = Cli::try_parse_from(["herdr-mem-cpu-load", "-g", "65"])
            .expect_err("65 cells does not fit a token");
        let message = error.to_string();
        assert!(message.contains("64 cell maximum"), "{message}");
        assert!(message.contains("80 characters"), "{message}");

        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "-g", "64"]).is_ok());
        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "--mem-graph-lines", "65"]).is_err());
        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "-g", "-1"]).is_err());
    }

    #[test]
    fn an_interval_longer_than_an_hour_is_rejected() {
        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "-i", "3600"]).is_ok());
        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "-i", "3601"]).is_err());
        // And the ttl still spans herdr's whole accepted range.
        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "--ttl-ms", "86400000"]).is_ok());
        assert!(Cli::try_parse_from(["herdr-mem-cpu-load", "--ttl-ms", "86400001"]).is_err());
    }

    #[test]
    fn daemon_mode_is_opt_in_and_defaults_to_the_sidebar_look() {
        let one_line = parse(&[]);
        assert!(!one_line.daemon.enabled);
        assert_eq!(settings(&[]).graph_style, GraphStyle::Classic);

        let options = settings(&["--daemon"]).daemon_options();
        assert_eq!(options.interval, Duration::from_secs(1));
        assert_eq!(options.ttl_ms, 3000);
        assert_eq!(options.source, "system-monitor");
        // The rows follow the focus unless a sidebar asks for them everywhere.
        assert_eq!(options.workspaces, WorkspaceScope::Focused);
        assert_eq!(options.history_len, 20);
        assert!(!options.verbose);
        assert_eq!(options.log, None);
        // The sidebar gets the unicode bar even though one-line mode does not.
        assert_eq!(options.tokens.graph_style, GraphStyle::Blocks);
    }

    #[test]
    fn daemon_flags_override_the_defaults() {
        let options = settings(&[
            "--daemon",
            "--interval",
            "2",
            "--ttl-ms",
            "9000",
            "--source",
            "my-monitor",
            "--history",
            "24",
            "--log-file",
            "/tmp/daemon.log",
            "--verbose",
            "--graph-style",
            "classic",
        ])
        .daemon_options();

        assert_eq!(options.interval, Duration::from_secs(2));
        assert_eq!(options.ttl_ms, 9000);
        assert_eq!(options.source, "my-monitor");
        assert_eq!(
            settings(&["--daemon", "--workspaces", "all"])
                .daemon_options()
                .workspaces,
            WorkspaceScope::All
        );
        assert_eq!(options.history_len, 24);
        assert!(options.verbose);
        assert_eq!(
            options.log.as_deref(),
            Some(std::path::Path::new("/tmp/daemon.log"))
        );
        assert_eq!(options.tokens.graph_style, GraphStyle::Classic);
    }

    #[test]
    fn the_default_ttl_follows_the_interval() {
        assert_eq!(
            settings(&["--daemon", "--interval", "2"])
                .daemon_options()
                .ttl_ms,
            5000
        );
    }

    #[test]
    fn the_colour_flags_pick_a_colour_mode() {
        assert_eq!(settings(&["-c"]).color, ColorMode::Tmux);
        assert_eq!(settings(&["--ansi"]).color, ColorMode::Ansi);

        let left = settings(&["-p", "-l", "4"]);
        assert_eq!(left.color, ColorMode::Tmux);
        assert_eq!(left.powerline, PowerlineMode::Left);
        assert_eq!(left.segments_left, Some(4));

        let right = settings(&["-q", "-r", "8"]);
        assert_eq!(right.color, ColorMode::Tmux);
        assert_eq!(right.powerline, PowerlineMode::Right);
        assert_eq!(right.segments_right, Some(8));

        // -q wins over -p, matching the original's if/else order.
        assert_eq!(settings(&["-p", "-q"]).powerline, PowerlineMode::Right);
        // --ansi wins over the tmux markup.
        assert_eq!(settings(&["-c", "--ansi"]).color, ColorMode::Ansi);
    }

    #[test]
    fn the_config_flags_are_parsed() {
        let cli = parse(&[
            "--config",
            "/tmp/hmcl.toml",
            "--print-config",
            "--write-default-config",
            "--force",
        ]);
        assert_eq!(
            cli.config_path(),
            Some(std::path::Path::new("/tmp/hmcl.toml"))
        );
        assert!(cli.print_config && cli.write_default_config && cli.force);
        assert!(parse(&[]).config_path().is_none());
    }
}
