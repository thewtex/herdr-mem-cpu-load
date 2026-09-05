//! `herdr-mem-cpu-load`: CPU, memory, and load average status.
//!
//! Without `--daemon` it prints the one-line output of the original
//! `tmux-mem-cpu-load` and exits. With `--daemon` it stays resident, sampling
//! on an interval and reporting Space sidebar tokens to a running herdr
//! server. With `--watch` it reprints the line in place, which is what the
//! plugin's status popup runs.

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
