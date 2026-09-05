//! `herdr-mem-cpu-load`: CPU, memory, and load average status.
//!
//! This binary is a thin wrapper over the `herdr_mem_cpu_load` library. It
//! parses the command line, merges it over the configuration file, and picks
//! one of four modes:
//!
//! * nothing — sample once, print the one line the original
//!   `tmux-mem-cpu-load` prints, and exit;
//! * `--daemon` — stay resident, sampling on an interval and reporting Space
//!   sidebar tokens to a running herdr server;
//! * `--watch` — reprint the line in place until interrupted, which is what
//!   the plugin's status popup pane runs;
//! * `--print-config` and `--write-default-config` — show or seed the
//!   configuration and exit.
//!
//! # The module map
//!
//! Everything below lives in the library, one layer per module, each built
//! only on the ones above it:
//!
//! * [`sys`](herdr_mem_cpu_load::sys) — the platform abstraction over the raw
//!   OS counters: `/proc` on Linux, Mach on macOS, Win32 on Windows, a stub
//!   that fails cleanly elsewhere. The only module holding any `unsafe`.
//! * [`metrics`](herdr_mem_cpu_load::metrics) — platform independent sampling
//!   on top of it: CPU deltas, memory, load averages, and the Windows load
//!   emulator.
//! * [`render`](herdr_mem_cpu_load::render) — bar graphs, the one-line text
//!   format, and the tmux and ANSI colour markup.
//! * [`tokens`](herdr_mem_cpu_load::tokens) — the pure function from a sample
//!   to the Space sidebar tokens, re-checking every limit herdr enforces.
//! * [`herdr`](herdr_mem_cpu_load::herdr) — the client that talks to a running
//!   herdr server through its CLI.
//! * [`daemon`](herdr_mem_cpu_load::daemon) — the sampling loop that ties
//!   those last two together, plus the singleton lock.
//! * [`cli`](herdr_mem_cpu_load::cli) — the `tmux-mem-cpu-load` compatible
//!   command line.
//! * [`config`](herdr_mem_cpu_load::config) — the `config.toml` layer the
//!   command line is merged over.
//! * [`watch`](herdr_mem_cpu_load::watch) — the live status line the herdr
//!   popup pane runs.

use std::fmt::Display;
use std::process::ExitCode;

use clap::Parser;

use herdr_mem_cpu_load::cli::Cli;
use herdr_mem_cpu_load::config::{self, Settings};
use herdr_mem_cpu_load::sys::SysError;
use herdr_mem_cpu_load::{daemon, metrics, render, watch};

fn main() -> ExitCode {
    let args = Cli::parse();

    // Writing the template comes first: it is the one mode that works before
    // there is a configuration file to read.
    if args.write_default_config {
        return write_default_config(&args);
    }

    let settings = args.settings();

    if args.print_config {
        return match settings.to_toml() {
            Ok(toml) => {
                print!("{toml}");
                ExitCode::SUCCESS
            }
            Err(error) => fail(&error),
        };
    }

    if args.daemon.enabled {
        daemon::run(&settings.daemon_options());
        return ExitCode::SUCCESS;
    }

    if args.watch {
        return match watch::run(&settings) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(&error),
        };
    }

    match run(&settings) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

/// Sample the system once and render the status line.
fn run(settings: &Settings) -> Result<String, SysError> {
    let cpu_percent = metrics::cpu::cpu_percentage(settings.sampling_delay())?;
    let sample = metrics::collect(cpu_percent)?;
    Ok(render::format::status_line(
        &sample,
        &settings.render_options(),
    ))
}

/// `--write-default-config`: drop a commented `config.toml` where the plugin
/// will look for it.
fn write_default_config(args: &Cli) -> ExitCode {
    let path = match config::default_config_target(args.config_path()) {
        Ok(path) => path,
        Err(message) => return fail(&message),
    };
    match config::write_default_config(&path, args.force) {
        Ok(()) => {
            println!("wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

fn fail(error: &dyn Display) -> ExitCode {
    eprintln!("herdr-mem-cpu-load: {error}");
    ExitCode::FAILURE
}
