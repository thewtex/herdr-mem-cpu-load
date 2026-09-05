//! Command line interface, kept flag-compatible with `tmux-mem-cpu-load` so
//! existing tmux configurations keep working.

use std::time::Duration;

use clap::{Args, Parser};

use crate::metrics::cpu::sampling_delay;
use crate::metrics::memory::MemoryMode;
use crate::metrics::CpuMode;
use crate::render::format::RenderOptions;
use crate::render::graph::GraphStyle;
use crate::sys::SysError;

/// CPU, memory, and load average monitor for herdr and tmux.
#[derive(Debug, Parser)]
#[command(name = "herdr-mem-cpu-load", version, about, long_about = None)]
pub struct Cli {
    /// Status refresh interval in seconds; also the CPU sampling window.
    #[arg(
        short = 'i',
        long,
        value_name = "SECS",
        default_value_t = 1,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub interval: u64,

    /// How many cells the CPU graph is drawn with. 0 hides the graph.
    #[arg(short = 'g', long, value_name = "N", default_value_t = 10)]
    pub graph_lines: usize,

    /// Memory display mode. 0: used/total, 1: free memory, 2: usage percent.
    #[arg(
        short = 'm',
        long,
        value_name = "0|1|2",
        default_value_t = 0,
        value_parser = clap::value_parser!(u8).range(0..=2)
    )]
    pub mem_mode: u8,

    /// CPU display mode. 0: max 100%, 1: max 100% per thread.
    #[arg(
        short = 't',
        long,
        value_name = "0|1",
        default_value_t = 0,
        value_parser = clap::value_parser!(u8).range(0..=1)
    )]
    pub cpu_mode: u8,

    /// How many load averages to print.
    #[arg(
        short = 'a',
        long,
        value_name = "0-3",
        default_value_t = 3,
        value_parser = clap::value_parser!(u8).range(0..=3)
    )]
    pub averages_count: u8,

    /// Use the single-character vertical bar chart for the CPU graph.
    #[arg(short = 'v', long)]
    pub vertical_graph: bool,

    /// CPU graph style: classic, blocks, or vertical.
    #[arg(long, value_name = "STYLE", default_value = "classic")]
    pub graph_style: GraphStyle,

    #[command(flatten)]
    pub compat: CompatibilityFlags,
}

/// Flags the original accepts that this port parses but does not act on yet.
///
/// They are kept so existing `tmux.conf` lines keep working unchanged; tmux
/// colour and powerline output are reserved for Phase 04.
#[derive(Debug, Args)]
pub struct CompatibilityFlags {
    /// Accepted and ignored; tmux colour output is reserved for Phase 04.
    #[arg(short = 'c', long)]
    pub colors: bool,

    /// Accepted and ignored; powerline output is reserved for Phase 04.
    #[arg(short = 'p', long)]
    pub powerline_left: bool,

    /// Accepted and ignored; powerline output is reserved for Phase 04.
    #[arg(short = 'q', long)]
    pub powerline_right: bool,

    /// Accepted and ignored; segment blending is reserved for Phase 04.
    #[arg(
        short = 'l',
        long,
        value_name = "COLOR",
        value_parser = clap::value_parser!(u16).range(0..=255)
    )]
    pub segments_left: Option<u16>,

    /// Accepted and ignored; segment blending is reserved for Phase 04.
    #[arg(
        short = 'r',
        long,
        value_name = "COLOR",
        value_parser = clap::value_parser!(u16).range(0..=255)
    )]
    pub segments_right: Option<u16>,
}

impl Cli {
    /// How long to sample the CPU for before printing.
    #[must_use]
    pub fn sampling_delay(&self) -> Duration {
        sampling_delay(self.interval)
    }

    /// The graph style, honouring the compatibility `-v` flag.
    #[must_use]
    pub const fn resolved_graph_style(&self) -> GraphStyle {
        if self.vertical_graph {
            GraphStyle::Vertical
        } else {
            self.graph_style
        }
    }

    /// Translate the numeric mode flags into the renderer's options.
    ///
    /// # Errors
    ///
    /// Returns a [`SysError`] when a mode value is out of range. In practice
    /// clap rejects those first; this is the belt to that suspenders.
    pub fn render_options(&self) -> Result<RenderOptions, SysError> {
        Ok(RenderOptions {
            mem_mode: MemoryMode::try_from(self.mem_mode)?,
            cpu_mode: CpuMode::try_from(self.cpu_mode)?,
            graph_style: self.resolved_graph_style(),
            graph_lines: self.graph_lines,
            averages_count: self.averages_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use crate::metrics::memory::MemoryMode;
    use crate::metrics::CpuMode;
    use crate::render::graph::GraphStyle;
    use clap::Parser;
    use std::time::Duration;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("herdr-mem-cpu-load").chain(args.iter().copied()))
            .expect("arguments parse")
    }

    #[test]
    fn defaults_match_the_original() {
        let cli = parse(&[]);
        let opts = cli.render_options().expect("default modes are valid");
        assert_eq!(opts.mem_mode, MemoryMode::Default);
        assert_eq!(opts.cpu_mode, CpuMode::Default);
        assert_eq!(opts.graph_style, GraphStyle::Classic);
        assert_eq!(opts.graph_lines, 10);
        assert_eq!(opts.averages_count, 3);
        assert_eq!(cli.sampling_delay(), Duration::from_millis(990));
    }

    #[test]
    fn vertical_flag_overrides_the_graph_style() {
        assert_eq!(parse(&["-v"]).resolved_graph_style(), GraphStyle::Vertical);
        assert_eq!(
            parse(&["--graph-style", "blocks"]).resolved_graph_style(),
            GraphStyle::Blocks
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
            vec!["-l", "256"],
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
    fn compatibility_flags_are_accepted() {
        let cli = parse(&["-c", "-p", "-q", "-l", "4", "-r", "8"]);
        let compat = &cli.compat;
        assert!(compat.colors && compat.powerline_left && compat.powerline_right);
        assert_eq!(compat.segments_left, Some(4));
        assert_eq!(compat.segments_right, Some(8));
    }
}
