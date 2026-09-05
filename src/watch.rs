//! `--watch`: reprint the status line in place until interrupted.
//!
//! This is what the plugin's `status` popup pane runs. It is deliberately not
//! a full-screen TUI: the line is rewritten with a carriage return and an
//! erase-to-end-of-line, so the output stays a single line that a pipe or a
//! `tee` still reads as text.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::daemon::LoadSource;
use crate::metrics::{self, cpu::CpuSampler};
use crate::render::format::status_line;
use crate::sys::{self, SysError};

/// Return to the start of the line and erase what was there.
const REDRAW: &str = "\r\x1b[K";
/// How long the loop sleeps at a stretch, so a closed stdin is noticed
/// promptly even with a long `--interval`.
const SLEEP_SLICE: Duration = Duration::from_millis(100);

/// Sample and reprint until Ctrl-C or, when stdin is a terminal, until it
/// closes.
///
/// # Errors
///
/// Returns a [`SysError`] when the system counters cannot be read.
pub fn run(settings: &Settings) -> Result<(), SysError> {
    run_into(&mut io::stdout(), settings)
}

/// [`run`] against an explicit sink, which is what the tests drive.
///
/// # Errors
///
/// Returns a [`SysError`] when the system counters cannot be read or the sink
/// cannot be written to.
pub fn run_into<W: Write>(out: &mut W, settings: &Settings) -> Result<(), SysError> {
    let stop = watch_stdin();
    let options = settings.render_options();
    let interval = settings.interval();

    // Reuse the daemon's sampler and load source rather than reimplementing
    // the tick: on Windows the load averages only exist because something
    // keeps feeding the emulator.
    let mut sampler = CpuSampler::new();
    let mut load = LoadSource::detect();
    let mut last_load_update: Option<Instant> = None;

    // The first snapshot is only a baseline; the CPU percentage needs two.
    sampler.sample()?;
    if !nap(settings.sampling_delay(), &stop) {
        return finish(out);
    }

    loop {
        let started = Instant::now();
        let percent = sampler.sample()?.unwrap_or(0.0);

        let now = Instant::now();
        let elapsed = last_load_update
            .replace(now)
            .map_or(interval, |previous| now.duration_since(previous));
        load.update(metrics::busy_cores(percent, sys::cpu_count()), elapsed);

        let sample = metrics::collect(percent)?;
        write!(out, "{REDRAW}{}", status_line(&sample, &options))
            .map_err(|error| write_error(&error))?;
        out.flush().map_err(|error| write_error(&error))?;

        let rest = interval.checked_sub(started.elapsed()).unwrap_or_default();
        if !nap(rest, &stop) {
            return finish(out);
        }
    }
}

/// Leave the cursor on a line of its own so a shell prompt does not land on
/// top of the last status.
fn finish<W: Write>(out: &mut W) -> Result<(), SysError> {
    writeln!(out).map_err(|error| write_error(&error))?;
    out.flush().map_err(|error| write_error(&error))
}

fn write_error(error: &io::Error) -> SysError {
    SysError::new(format!("could not write the status line: {error}"))
}

/// Sleep for `total`, waking early if `stop` is set. Returns whether the loop
/// should keep going.
fn nap(total: Duration, stop: &AtomicBool) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return true;
        };
        std::thread::sleep(left.min(SLEEP_SLICE));
    }
}

/// Watch for stdin closing, which is how a popup pane going away stops the
/// loop.
///
/// Only a terminal is watched. A `--watch` run with its stdin redirected from
/// `/dev/null` would otherwise see EOF immediately and quit before printing
/// anything, which is the opposite of what a watch command should do.
fn watch_stdin() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    if !io::stdin().is_terminal() {
        return stop;
    }

    let flag = Arc::clone(&stop);
    // Detached on purpose: the read blocks until the terminal goes away, and
    // process exit is what tears the thread down.
    let _ = std::thread::Builder::new()
        .name("watch-stdin".to_string())
        .spawn(move || {
            let mut byte = [0_u8; 1];
            loop {
                match io::Read::read(&mut io::stdin(), &mut byte) {
                    // EOF: the terminal closed.
                    Ok(0) | Err(_) => break,
                    // `q` is the obvious thing to press in a popup.
                    Ok(_) if byte[0] == b'q' => break,
                    Ok(_) => {}
                }
            }
            flag.store(true, Ordering::Relaxed);
        });
    stop
}

#[cfg(test)]
mod tests {
    use super::{nap, run_into, REDRAW};
    use crate::config::Settings;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn a_set_stop_flag_ends_the_nap_immediately() {
        let stop = AtomicBool::new(true);
        let started = Instant::now();
        assert!(!nap(Duration::from_secs(30), &stop));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn an_unset_flag_sleeps_the_whole_time() {
        let stop = AtomicBool::new(false);
        assert!(nap(Duration::from_millis(1), &stop));
        assert!(nap(Duration::ZERO, &stop));
    }

    /// A sink that refuses every write, which is the only way to make the
    /// endless loop in `run_into` return.
    struct Closed;

    impl std::io::Write for Closed {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_line_is_redrawn_in_place_rather_than_appended() {
        // A carriage return and an erase-to-end-of-line, so the output stays
        // one line however wide the previous sample was.
        assert_eq!(REDRAW, "\r\x1b[K");
    }

    #[test]
    fn a_sink_that_cannot_be_written_to_stops_the_loop() {
        let settings = Settings {
            // Keep the sampling delay short: the loop measures the CPU before
            // it prints anything.
            interval_secs: 1,
            ..Settings::default()
        };
        let error = run_into(&mut Closed, &settings).expect_err("a closed sink stops the loop");
        assert!(
            error
                .to_string()
                .contains("could not write the status line"),
            "{error}"
        );
    }
}
