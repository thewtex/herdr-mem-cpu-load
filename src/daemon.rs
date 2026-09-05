//! Daemon mode: sample on an interval and report Space sidebar tokens for
//! every open workspace.
//!
//! The loop is deliberately quiet. herdr copies a plugin's stdout and stderr
//! into a capped command log, so a process that printed a line every two
//! seconds would fill it within the hour. Nothing is written to stdout at all;
//! diagnostics go to a log file when one is configured.

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::herdr::{HerdrCli, MetadataReport};
#[cfg(windows)]
use crate::metrics::load_emulator::LoadEmulator;
use crate::metrics::{self, cpu::CpuSampler};
use crate::render::format::status_line;
use crate::sys;
use crate::tokens::{build_tokens, TokenOptions, TokenSet};

/// How long the daemon runs before giving up on a herdr that never answers.
pub const DEFAULT_MAX_FAILURES: u32 = 5;
/// The default sampling interval.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(2);
/// The log file name used under `HERDR_PLUGIN_STATE_DIR`.
pub const LOG_FILE_NAME: &str = "daemon.log";

/// How the daemon samples and reports.
#[derive(Clone, Debug, PartialEq)]
pub struct DaemonOptions {
    /// Time between samples; also the CPU measurement window.
    pub interval: Duration,
    /// How long herdr keeps the tokens without a refresh. Slightly more than
    /// two intervals, so a stopped daemon's rows disappear on their own.
    pub ttl_ms: u64,
    /// The metadata source id every report is attributed to.
    pub source: String,
    /// What the tokens look like.
    pub tokens: TokenOptions,
    /// How many CPU samples the `cpu_history` sparkline keeps.
    pub history_len: usize,
    /// Consecutive `workspace list` failures that mean herdr is gone.
    pub max_failures: u32,
    /// Where to append diagnostics.
    pub log: Option<PathBuf>,
    /// Log every tick's status line, not just errors.
    pub verbose: bool,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            interval: DEFAULT_INTERVAL,
            ttl_ms: default_ttl_ms(DEFAULT_INTERVAL),
            source: "system-monitor".to_string(),
            tokens: TokenOptions::default(),
            history_len: TokenOptions::default().graph_lines,
            max_failures: DEFAULT_MAX_FAILURES,
            log: None,
            verbose: false,
        }
    }
}

/// The token lifetime for an interval: two ticks plus a second of slack, so a
/// single slow tick does not blink the sidebar rows out of existence.
#[must_use]
pub fn default_ttl_ms(interval: Duration) -> u64 {
    let interval_ms = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
    interval_ms.saturating_mul(2).saturating_add(1000)
}

/// Unix time in milliseconds, the daemon's clock for sequence numbers and log
/// timestamps.
#[must_use]
pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Monotonic `--seq` values.
///
/// herdr ignores a report whose sequence number is at or below the last one it
/// accepted from the same source, and it remembers that number across daemon
/// restarts. Unix milliseconds therefore make a better sequence than a counter
/// starting at zero; the generator only has to guarantee that a frozen or
/// backwards-stepping clock still produces increasing values.
#[derive(Clone, Debug)]
pub struct SeqGenerator<C> {
    clock: C,
    last: u64,
}

impl<C: FnMut() -> u64> SeqGenerator<C> {
    /// A generator reading `clock`, which returns Unix milliseconds.
    pub const fn new(clock: C) -> Self {
        Self { clock, last: 0 }
    }

    /// The next sequence number, always greater than the previous one.
    pub fn next_seq(&mut self) -> u64 {
        let now = (self.clock)();
        let seq = if now > self.last {
            now
        } else {
            self.last.saturating_add(1)
        };
        self.last = seq;
        seq
    }
}

impl Default for SeqGenerator<fn() -> u64> {
    fn default() -> Self {
        Self::new(unix_millis)
    }
}

/// A fixed-size ring of recent CPU percentages, oldest first.
#[derive(Clone, Debug, Default)]
pub struct History {
    values: VecDeque<f32>,
    capacity: usize,
}

impl History {
    /// A ring holding at most `capacity` values. A capacity of zero keeps
    /// nothing, which clears the `cpu_history` token.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Append a sample, dropping the oldest once the ring is full.
    pub fn push(&mut self, value: f32) {
        if self.capacity == 0 {
            return;
        }
        while self.values.len() >= self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    /// The retained values, oldest first.
    #[must_use]
    pub fn snapshot(&self) -> Vec<f32> {
        self.values.iter().copied().collect()
    }

    /// How many values are retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the ring is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Appends timestamped lines to the daemon log, if there is one.
#[derive(Clone, Debug, Default)]
struct Logger {
    path: Option<PathBuf>,
}

impl Logger {
    /// `--log-file` wins; otherwise herdr's per-plugin state directory gets a
    /// `daemon.log`. With neither, the daemon is silent.
    fn new(options: &DaemonOptions) -> Self {
        let path = options.log.clone().or_else(|| {
            std::env::var_os("HERDR_PLUGIN_STATE_DIR")
                .filter(|dir| !dir.is_empty())
                .map(|dir| PathBuf::from(dir).join(LOG_FILE_NAME))
        });
        Self { path }
    }

    /// Append one line. Every failure is swallowed: a full disk or a closed
    /// pipe must not take the sampler down with it.
    fn log(&self, message: &str) {
        let Some(path) = &self.path else {
            return;
        };
        let line = format!("{} {message}\n", format_timestamp(unix_millis()));
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

/// Where the daemon's load averages come from.
///
/// Every platform but Windows has a kernel load average, and on those the
/// daemon does nothing at all. Windows has none, so the daemon runs a
/// [`LoadEmulator`](crate::metrics::load_emulator::LoadEmulator) over the CPU
/// percentages it is already measuring and publishes the result for
/// `sys::load_averages` to report.
///
/// Each platform gets exactly the variant it can use, so the Linux and macOS
/// code path stays a no-op the optimiser deletes.
#[derive(Clone, Debug)]
enum LoadSource {
    /// The kernel keeps the averages; there is nothing to feed.
    #[cfg(not(windows))]
    Native,
    /// The daemon keeps the averages itself.
    #[cfg(windows)]
    Emulated(LoadEmulator),
}

impl LoadSource {
    /// The source this platform needs.
    fn detect() -> Self {
        #[cfg(windows)]
        {
            Self::Emulated(LoadEmulator::new())
        }
        #[cfg(not(windows))]
        {
            Self::Native
        }
    }

    /// Fold this tick's reading in, before anything reads the load averages
    /// back out.
    fn update(&mut self, busy_cores: f64, dt: Duration) {
        match self {
            #[cfg(not(windows))]
            Self::Native => {
                let _ = (busy_cores, dt);
            }
            #[cfg(windows)]
            Self::Emulated(emulator) => {
                sys::publish_emulated_load(emulator.update(busy_cores, dt));
            }
        }
    }
}

/// Whether one tick should be followed by another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

/// The daemon's mutable state, kept together so a tick is one method call.
struct Daemon<'a> {
    options: &'a DaemonOptions,
    logger: Logger,
    cli: HerdrCli,
    sampler: CpuSampler,
    history: History,
    load: LoadSource,
    /// When the load source was last fed, so its decay uses the real elapsed
    /// time rather than the nominal interval.
    last_load_update: Option<Instant>,
    seq: SeqGenerator<fn() -> u64>,
    failures: u32,
}

impl<'a> Daemon<'a> {
    fn new(options: &'a DaemonOptions) -> Self {
        Self {
            options,
            logger: Logger::new(options),
            cli: HerdrCli::from_env(),
            sampler: CpuSampler::new(),
            history: History::new(options.history_len),
            load: LoadSource::detect(),
            last_load_update: None,
            seq: SeqGenerator::default(),
            failures: 0,
        }
    }

    /// Sample, report, and say whether to keep going.
    fn tick(&mut self) -> Flow {
        if herdr_socket_is_gone() {
            self.logger
                .log("the herdr socket disappeared, shutting down");
            return Flow::Stop;
        }

        let cpu_percent = match self.sampler.sample() {
            // The first tick only establishes the baseline the next delta needs.
            Ok(None) => return Flow::Continue,
            Ok(Some(percent)) => percent,
            Err(error) => {
                self.logger.log(&format!("cpu sampling failed: {error}"));
                return Flow::Continue;
            }
        };

        self.feed_load_source(cpu_percent);

        let sample = match metrics::collect(cpu_percent) {
            Ok(sample) => sample,
            Err(error) => {
                self.logger.log(&format!("sampling failed: {error}"));
                return Flow::Continue;
            }
        };
        self.history.push(sample.cpu_percent);
        let tokens = build_tokens(&sample, &self.history.snapshot(), &self.options.tokens);

        let workspaces = match self.cli.list_workspaces() {
            Ok(workspaces) => {
                self.failures = 0;
                workspaces
            }
            Err(error) => {
                self.failures = self.failures.saturating_add(1);
                self.logger.log(&format!(
                    "workspace list failed ({}/{}): {error}",
                    self.failures, self.options.max_failures
                ));
                if self.failures >= self.options.max_failures {
                    self.logger.log("herdr is not answering, shutting down");
                    return Flow::Stop;
                }
                return Flow::Continue;
            }
        };

        if self.options.verbose {
            self.logger.log(&format!(
                "{} ({} workspace(s))",
                status_line(&sample, &self.options.tokens.render_options()),
                workspaces.len()
            ));
        }

        let seq = self.seq.next_seq();
        for workspace in &workspaces {
            let report = self.report_for(&workspace.workspace_id, &tokens, seq);
            // A workspace can close between the list and the report; that is
            // one lost row, not a reason to stop.
            if let Err(error) = self.cli.report_metadata(&report) {
                self.logger.log(&format!(
                    "reporting to {} failed: {error}",
                    workspace.workspace_id
                ));
            }
        }
        Flow::Continue
    }

    /// Hand this tick's reading to the load source, measuring the real gap
    /// since the previous one so a late tick decays by the right amount.
    fn feed_load_source(&mut self, cpu_percent: f32) {
        let now = Instant::now();
        let dt = self
            .last_load_update
            .replace(now)
            .map_or(self.options.interval, |previous| {
                now.duration_since(previous)
            });
        self.load
            .update(metrics::busy_cores(cpu_percent, sys::cpu_count()), dt);
    }

    fn report_for(&self, workspace_id: &str, tokens: &TokenSet, seq: u64) -> MetadataReport {
        MetadataReport {
            workspace_id: workspace_id.to_string(),
            source: self.options.source.clone(),
            set: tokens.set.clone(),
            clear: tokens.clear.clone(),
            ttl_ms: self.options.ttl_ms,
            seq,
        }
    }
}

/// Run the sampler until herdr goes away.
///
/// Returns once the server is gone: either its socket vanished or
/// `workspace list` failed `max_failures` times in a row. Both are ordinary
/// shutdowns, not errors.
pub fn run(options: &DaemonOptions) {
    let mut daemon = Daemon::new(options);
    daemon.logger.log(&format!(
        "herdr-mem-cpu-load {} started: interval {} ms, ttl {} ms, source {}, history {}",
        env!("CARGO_PKG_VERSION"),
        options.interval.as_millis(),
        options.ttl_ms,
        options.source,
        options.history_len,
    ));

    loop {
        let started = Instant::now();
        if daemon.tick() == Flow::Stop {
            return;
        }
        // Measure the sleep from the start of the tick so the cost of talking
        // to herdr does not make the schedule drift.
        if let Some(rest) = options.interval.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

/// Whether the Unix socket herdr told the plugin about has been removed, which
/// is how a shut down server is noticed without waiting for the failure count.
///
/// On Windows `HERDR_SOCKET_PATH` names a pipe rather than a file, so there is
/// nothing to stat and the check is skipped.
#[cfg(unix)]
fn herdr_socket_is_gone() -> bool {
    std::env::var_os("HERDR_SOCKET_PATH")
        .filter(|path| !path.is_empty())
        .is_some_and(|path| !std::path::Path::new(&path).exists())
}

#[cfg(not(unix))]
fn herdr_socket_is_gone() -> bool {
    false
}

/// `2026-09-05T21:07:40.512Z` from Unix milliseconds, without pulling in a date
/// library for one log prefix.
#[must_use]
pub fn format_timestamp(millis: u64) -> String {
    let seconds = millis / 1000;
    let (year, month, day) = civil_from_days(seconds / 86_400);
    let time = seconds % 86_400;
    let (hour, minute, second) = (time / 3600, time % 3600 / 60, time % 60);
    let fraction = millis % 1000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction:03}Z")
}

/// Howard Hinnant's `civil_from_days`, restricted to days after the epoch.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{default_ttl_ms, format_timestamp, DaemonOptions, History, SeqGenerator};
    use std::cell::Cell;
    use std::time::Duration;

    #[test]
    fn sequence_numbers_increase_even_when_the_clock_is_frozen() {
        let mut seq = SeqGenerator::new(|| 1_700_000_000_000_u64);
        let first = seq.next_seq();
        let second = seq.next_seq();
        let third = seq.next_seq();

        assert_eq!(first, 1_700_000_000_000);
        assert_eq!(second, 1_700_000_000_001);
        assert_eq!(third, 1_700_000_000_002);
        assert!(first < second && second < third);
    }

    #[test]
    fn sequence_numbers_follow_a_moving_clock_and_survive_it_going_backwards() {
        let now = Cell::new(1_700_000_000_000_u64);
        let mut seq = SeqGenerator::new(|| now.get());

        assert_eq!(seq.next_seq(), 1_700_000_000_000);
        now.set(1_700_000_002_000);
        assert_eq!(seq.next_seq(), 1_700_000_002_000);
        // A clock step backwards must not let herdr reject the report.
        now.set(1_600_000_000_000);
        assert_eq!(seq.next_seq(), 1_700_000_002_001);
    }

    #[test]
    fn the_history_ring_keeps_only_the_last_values() {
        let mut history = History::new(3);
        assert!(history.is_empty());

        for value in [1.0, 2.0, 3.0, 4.0, 5.0] {
            history.push(value);
        }
        assert_eq!(history.len(), 3);
        assert_eq!(history.snapshot(), vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn a_zero_length_history_keeps_nothing() {
        let mut history = History::new(0);
        history.push(42.0);
        assert!(history.is_empty());
        assert!(history.snapshot().is_empty());
    }

    #[test]
    fn the_default_ttl_outlives_two_intervals() {
        assert_eq!(default_ttl_ms(Duration::from_secs(2)), 5000);
        assert_eq!(default_ttl_ms(Duration::from_secs(1)), 3000);
        assert_eq!(DaemonOptions::default().ttl_ms, 5000);
        assert_eq!(DaemonOptions::default().source, "system-monitor");
        assert_eq!(DaemonOptions::default().history_len, 10);
    }

    #[test]
    fn timestamps_are_formatted_as_utc() {
        // 2026-09-05T21:07:40.512Z
        assert_eq!(
            format_timestamp(1_788_642_460_512),
            "2026-09-05T21:07:40.512Z"
        );
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00.000Z");
    }
}
