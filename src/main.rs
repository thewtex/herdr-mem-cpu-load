//! `herdr-mem-cpu-load`: CPU, memory, and load average status line.
//!
//! Phase 01 provides the one-line output of the original `tmux-mem-cpu-load`.

use std::process::ExitCode;

use clap::Parser;

use herdr_mem_cpu_load::cli::Cli;
use herdr_mem_cpu_load::sys::SysError;
use herdr_mem_cpu_load::{metrics, render};

fn main() -> ExitCode {
    let args = Cli::parse();
    match run(&args) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("herdr-mem-cpu-load: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Sample the system once and render the status line.
fn run(args: &Cli) -> Result<String, SysError> {
    let options = args.render_options()?;
    let cpu_percent = metrics::cpu::cpu_percentage(args.sampling_delay())?;
    let sample = metrics::collect(cpu_percent)?;
    Ok(render::format::status_line(&sample, &options))
}
