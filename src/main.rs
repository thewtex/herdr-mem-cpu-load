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
//! * `--print-config`, `--write-default-config`, and
//!   `--write-sidebar-rows` — show or seed configuration and exit.
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
//!   command line is merged over, and the watcher that re-runs the merge for
//!   a running daemon when the file changes.
//! * [`sidebar`](herdr_mem_cpu_load::sidebar) — the rows this plugin's tokens
//!   need, seeded into herdr's own `config.toml` at install time.
//! * [`watch`](herdr_mem_cpu_load::watch) — the live status line the herdr
//!   popup pane runs.

use std::fmt::Display;
use std::process::ExitCode;

use clap::Parser;

use herdr_mem_cpu_load::cli::Cli;
use herdr_mem_cpu_load::config::{self, Settings};
use herdr_mem_cpu_load::sys::SysError;
use herdr_mem_cpu_load::{daemon, metrics, render, sidebar, watch};

fn main() -> ExitCode {
    let args = Cli::parse();

    // Writing the template comes first: it is the one mode that works before
    // there is a configuration file to read.
    if args.write_default_config {
        return write_default_config(&args);
    }

    // Seeding the sidebar comes next, and for the same reason: it is about
    // herdr's configuration rather than this plugin's, so it reads none of
    // its own.
    if args.write_sidebar_rows {
        return write_sidebar_rows();
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
        // The daemon outlives any number of edits to the file it started
        // from, so it keeps watching that file and re-resolves the whole merge
        // whenever it changes.
        let mut watcher = config::ConfigWatcher::new(&args);
        let mut reload = || watcher.poll().map(|settings| settings.daemon_options());
        daemon::run(settings.daemon_options(), Some(&mut reload));
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

/// `--write-sidebar-rows`: put this plugin's rows in herdr's `config.toml`
/// when no layout has claimed `[ui.sidebar.spaces]` yet.
///
/// The plugin's install runs this, and an install must not fail over a file
/// this plugin does not own, so every outcome is a success: a configuration
/// that cannot be read, does not parse, or is not ours to edit is reported
/// and left alone, with the rows to add named in the message.
fn write_sidebar_rows() -> ExitCode {
    let path = sidebar::config_path();
    match sidebar::write_rows(&path) {
        Ok(sidebar::Outcome::Written) => {
            println!("added the sidebar rows to {}", path.display());
        }
        Ok(sidebar::Outcome::AlreadySet) => {
            println!(
                "{} already lays out [ui.sidebar.spaces]; leaving it alone",
                path.display()
            );
        }
        Err(error) => {
            eprintln!("herdr-mem-cpu-load: {error}");
            eprintln!("add the rows by hand to keep the sidebar in step:");
            eprint!("\n[ui.sidebar.spaces]\n{}", sidebar::SPACES_ROWS);
        }
    }
    ExitCode::SUCCESS
}

fn fail(error: &dyn Display) -> ExitCode {
    eprintln!("herdr-mem-cpu-load: {error}");
    ExitCode::FAILURE
}
