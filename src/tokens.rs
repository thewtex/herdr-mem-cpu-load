//! Space sidebar tokens.
//!
//! This module is a pure function from a [`Sample`] to the names and values
//! herdr should display, with no knowledge of how the report is delivered.
//! Everything herdr enforces on a metadata report — how many keys it may
//! mention, how long a value may be, which characters a key may use — is
//! re-checked here so a bad report is a test failure rather than a rejected
//! CLI call.

use crate::metrics::load::load_per_core;
use crate::metrics::memory::MemoryMode;
use crate::metrics::{CpuMode, Sample};
use crate::render::colors::{ColorMode, PowerlineMode};
use crate::render::format::{cpu_text, load_text, mem_text, status_line, RenderOptions};
use crate::render::graph::{render_bar, sparkline, GraphStyle};

/// herdr accepts at most 16 token keys in one metadata report.
pub const MAX_TOKEN_KEYS: usize = 16;
/// herdr caps a token value at 80 characters (`char`s, not bytes).
pub const MAX_TOKEN_VALUE_CHARS: usize = 80;
/// herdr caps a token key at 32 characters.
pub const MAX_TOKEN_KEY_CHARS: usize = 32;
/// The characters a `classic` or `blocks` bar spends on its brackets or frame.
const BAR_FRAME_CHARS: usize = 2;

/// The metrics that get a level token, in report order.
const METRICS: [&str; 3] = ["cpu", "mem", "load"];

/// Where each metric stops being comfortable and starts being interesting.
///
/// CPU and memory are percentages; load is the one minute average divided by
/// the CPU count, so `1.0` means "the machine is exactly saturated".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub cpu_warn: f32,
    pub cpu_hot: f32,
    pub mem_warn: f32,
    pub mem_hot: f32,
    pub load_warn: f64,
    pub load_hot: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            cpu_warn: 50.0,
            cpu_hot: 80.0,
            mem_warn: 70.0,
            mem_hot: 90.0,
            load_warn: 0.7,
            load_hot: 1.0,
        }
    }
}

/// How loaded a metric is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Ok,
    Warn,
    Hot,
}

impl Level {
    /// Every level, in the order the tokens are emitted.
    pub const ALL: [Self; 3] = [Self::Ok, Self::Warn, Self::Hot];

    /// The token name suffix for this level, as in `cpu_warn`.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Hot => "hot",
        }
    }
}

/// Bucket `value` into a [`Level`]: hot at or above `hot`, warn at or above
/// `warn`, otherwise ok.
#[must_use]
pub fn classify(value: f64, warn: f64, hot: f64) -> Level {
    if value >= hot {
        Level::Hot
    } else if value >= warn {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// How far below a threshold a value has to fall before the level drops back.
///
/// Without it a machine hovering on a threshold repaints the sidebar row a
/// different colour every tick. CPU and memory are percentages, so the margin
/// is in percentage points; load is per core, where 0.1 is a tenth of a core.
pub const PERCENT_MARGIN: f64 = 5.0;
/// The hysteresis margin for the per-core load average.
pub const LOAD_MARGIN: f64 = 0.1;

/// The next [`Level`] for a metric that is currently at `current`.
///
/// Escalation is immediate — the moment a value reaches a threshold the level
/// rises — but de-escalation waits until the value has fallen `margin` below
/// the threshold it came from.
#[must_use]
pub fn next(current: Level, value: f64, warn: f64, hot: f64, margin: f64) -> Level {
    if value >= hot || (current == Level::Hot && value >= hot - margin) {
        Level::Hot
    } else if value >= warn || (current != Level::Ok && value >= warn - margin) {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// The level each metric was last reported at, so [`next`] can apply
/// hysteresis across ticks.
///
/// A fresh tracker starts every metric at [`Level::Ok`], where [`next`] and
/// [`classify`] agree: nothing can de-escalate out of `Ok`, so the first sample
/// is plainly classified.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelTracker {
    cpu: Level,
    mem: Level,
    load: Level,
}

impl LevelTracker {
    /// A tracker with every metric at [`Level::Ok`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            cpu: Level::Ok,
            mem: Level::Ok,
            load: Level::Ok,
        }
    }

    /// The CPU, memory, and load levels for this sample, in report order.
    fn advance(
        &mut self,
        cpu_percent: f64,
        mem_percent: f64,
        load: f64,
        thresholds: &Thresholds,
    ) -> [Level; 3] {
        self.cpu = next(
            self.cpu,
            cpu_percent,
            f64::from(thresholds.cpu_warn),
            f64::from(thresholds.cpu_hot),
            PERCENT_MARGIN,
        );
        self.mem = next(
            self.mem,
            mem_percent,
            f64::from(thresholds.mem_warn),
            f64::from(thresholds.mem_hot),
            PERCENT_MARGIN,
        );
        self.load = next(
            self.load,
            load,
            thresholds.load_warn,
            thresholds.load_hot,
            LOAD_MARGIN,
        );
        [self.cpu, self.mem, self.load]
    }
}

/// Everything the token builder needs beyond the sample.
///
/// The defaults are the daemon's, which differ from one-line mode in one place:
/// the sidebar gets the unicode [`GraphStyle::Blocks`] bar rather than the
/// ASCII bar the original `tmux-mem-cpu-load` prints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenOptions {
    pub graph_style: GraphStyle,
    pub graph_lines: usize,
    /// Cells in the memory bar. Memory moves far less than the CPU does, so a
    /// sidebar often wants a shorter bar for it.
    pub mem_graph_lines: usize,
    /// Cells in the load bar, which is the one minute load per core clamped to
    /// one saturated machine. Load is the coarsest of the three readings, so
    /// it is the one most often given a bar of its own.
    pub load_graph_lines: usize,
    pub mem_mode: MemoryMode,
    pub cpu_mode: CpuMode,
    pub averages_count: u8,
    pub thresholds: Thresholds,
}

impl Default for TokenOptions {
    fn default() -> Self {
        Self {
            graph_style: GraphStyle::Blocks,
            graph_lines: 10,
            mem_graph_lines: 10,
            load_graph_lines: 4,
            mem_mode: MemoryMode::Default,
            cpu_mode: CpuMode::Default,
            averages_count: 3,
            thresholds: Thresholds::default(),
        }
    }
}

impl TokenOptions {
    /// These options with every bar width narrowed until each token fits
    /// inside [`MAX_TOKEN_VALUE_CHARS`].
    ///
    /// Something has to give when a wide bar and a long reading do not both
    /// fit in one token. Truncating cuts the percentage off the end, which is
    /// the half a reader actually needs; narrowing the bar costs a cell of a
    /// graph that is approximate anyway. `sys_status` is the binding
    /// constraint: it carries all three segments and the bar at once.
    ///
    /// One budget is applied to all three bars rather than one each, so the
    /// CPU, memory, and load rows still line up under each other in the
    /// sidebar.
    /// [`GraphStyle::Vertical`] is one character wide whatever `graph_lines`
    /// says, so there is nothing to narrow for it.
    #[must_use]
    fn fitted(self, mem: &str, cpu: &str, load: &str) -> Self {
        if self.graph_style == GraphStyle::Vertical {
            return self;
        }
        let width = |text: &str| text.chars().count();
        // `cpu_status`, `mem_status`, and `load_status` are a bar, a space,
        // and the trimmed text. `sys_status` is the three segments run
        // together, with the classic bar alone taking a leading space.
        let widest = [
            1 + width(cpu.trim()),
            1 + width(mem),
            1 + width(load.trim()),
            width(mem)
                + usize::from(self.graph_style == GraphStyle::Classic)
                + width(cpu)
                + width(load),
        ]
        .into_iter()
        .max()
        .unwrap_or(0);

        let cells = MAX_TOKEN_VALUE_CHARS.saturating_sub(widest + BAR_FRAME_CHARS);
        Self {
            graph_lines: self.graph_lines.min(cells),
            mem_graph_lines: self.mem_graph_lines.min(cells),
            load_graph_lines: self.load_graph_lines.min(cells),
            ..self
        }
    }

    /// The one-line renderer's options, used for the combined `sys_status`.
    ///
    /// Sidebar tokens carry no colour markup of their own: herdr styles a row
    /// from the config, and a token value may not contain control characters.
    #[must_use]
    pub const fn render_options(&self) -> RenderOptions {
        RenderOptions {
            mem_mode: self.mem_mode,
            cpu_mode: self.cpu_mode,
            graph_style: self.graph_style,
            graph_lines: self.graph_lines,
            averages_count: self.averages_count,
            color: ColorMode::None,
            powerline: PowerlineMode::None,
            segments_left: None,
            segments_right: None,
        }
    }
}

/// The tokens one report sets and clears.
///
/// Cleared keys matter as much as set ones: herdr drops a token without a value
/// along with its separator, which is what lets a sidebar row list all three
/// level tokens and show only the one that currently applies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenSet {
    pub set: Vec<(String, String)>,
    pub clear: Vec<String>,
}

impl TokenSet {
    fn assign(&mut self, key: &str, value: &str) {
        self.set.push((
            key.to_string(),
            truncate_chars(value, MAX_TOKEN_VALUE_CHARS),
        ));
    }

    fn drop_key(&mut self, key: &str) {
        self.clear.push(key.to_string());
    }

    /// How many keys this report mentions.
    #[must_use]
    pub fn mentioned_keys(&self) -> usize {
        self.set.len() + self.clear.len()
    }

    /// Every key this report mentions, set or cleared.
    ///
    /// A tick mentions all of them, so this is the whole vocabulary the daemon
    /// publishes: what a report has to clear to leave a workspace as it found
    /// it.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.set
            .iter()
            .map(|(key, _)| key.clone())
            .chain(self.clear.iter().cloned())
            .collect()
    }

    /// The value of `key`, if this report sets one.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&str> {
        self.set
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Whether this report clears `key`.
    #[must_use]
    pub fn clears(&self, key: &str) -> bool {
        self.clear.iter().any(|name| name == key)
    }

    /// Check the report against every limit herdr enforces.
    ///
    /// # Errors
    ///
    /// Returns a description of the first violated limit.
    pub fn validate(&self) -> Result<(), String> {
        if self.mentioned_keys() > MAX_TOKEN_KEYS {
            return Err(format!(
                "a report may mention at most {MAX_TOKEN_KEYS} keys, this one mentions {}",
                self.mentioned_keys()
            ));
        }
        for key in self.set.iter().map(|(key, _)| key).chain(self.clear.iter()) {
            if !is_valid_key(key) {
                return Err(format!("invalid token key `{key}`"));
            }
        }
        for (key, value) in &self.set {
            let length = value.chars().count();
            if length > MAX_TOKEN_VALUE_CHARS {
                return Err(format!(
                    "`{key}` is {length} characters, over the {MAX_TOKEN_VALUE_CHARS} character limit"
                ));
            }
            if value.chars().any(char::is_control) {
                return Err(format!("`{key}` contains a control character"));
            }
        }
        Ok(())
    }
}

/// Whether `key` matches herdr's `[A-Za-z0-9_-]{1,32}` token key rule.
#[must_use]
pub fn is_valid_key(key: &str) -> bool {
    let length = key.chars().count();
    length != 0
        && length <= MAX_TOKEN_KEY_CHARS
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

/// Keep the first `max` characters of `value`.
///
/// Cutting on characters rather than bytes is what herdr does, and it is the
/// only way to truncate a bar of block characters without splitting one.
#[must_use]
pub fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Build every Space sidebar token for one sample.
///
/// `history` is the recent CPU percentages, oldest first, used for the
/// `cpu_history` sparkline; an empty history clears that token. `levels`
/// carries the previous tick's levels so a metric sitting on a threshold does
/// not flicker between two colours.
#[must_use]
pub fn build_tokens(
    sample: &Sample,
    history: &[f32],
    opts: &TokenOptions,
    levels: &mut LevelTracker,
) -> TokenSet {
    let mut tokens = TokenSet::default();

    // The text comes first: how wide a bar can be depends on how much room
    // the reading beside it leaves.
    let cpu_reading = cpu_text(sample.cpu_percent, opts.cpu_mode, sample.cpu_count);
    let mem_reading = mem_text(&sample.memory, opts.mem_mode);
    let load_reading = load_text(&sample.load, opts.averages_count);
    let opts = &opts.fitted(&mem_reading, &cpu_reading, &load_reading);

    let cpu_status = format!(
        "{} {}",
        render_bar(opts.graph_style, sample.cpu_percent, opts.graph_lines),
        cpu_reading.trim()
    );
    tokens.assign("cpu_status", &cpu_status);

    let mem_percent = sample.memory.used_percent();
    let mem_status = format!(
        "{} {}",
        render_bar(opts.graph_style, mem_percent, opts.mem_graph_lines),
        mem_reading
    );
    tokens.assign("mem_status", &mem_status);

    let load = load_per_core(&sample.load, sample.cpu_count);
    let load_status = (opts.averages_count > 0).then(|| {
        format!(
            "{} {}",
            render_bar(
                opts.graph_style,
                (load * 100.0).min(100.0) as f32,
                opts.load_graph_lines
            ),
            load_reading.trim()
        )
    });
    match &load_status {
        Some(status) => tokens.assign("load_status", status),
        None => tokens.drop_key("load_status"),
    }

    tokens.assign("sys_status", &status_line(sample, &opts.render_options()));

    if history.is_empty() {
        tokens.drop_key("cpu_history");
    } else {
        tokens.assign("cpu_history", &sparkline(history));
    }

    let current = levels.advance(
        f64::from(sample.cpu_percent),
        f64::from(mem_percent),
        load,
        &opts.thresholds,
    );
    let statuses = [Some(cpu_status), Some(mem_status), load_status];
    for ((metric, level), status) in METRICS.iter().zip(current).zip(&statuses) {
        push_level_tokens(&mut tokens, metric, level, status.as_deref());
    }

    debug_assert!(
        tokens.validate().is_ok(),
        "build_tokens broke a herdr metadata limit: {:?}",
        tokens.validate().err()
    );
    tokens
}

/// Set the level token that applies and clear the other two.
///
/// A metric without a status line — load with `--averages-count 0` — clears all
/// three, because there is nothing to colour.
fn push_level_tokens(tokens: &mut TokenSet, metric: &str, level: Level, status: Option<&str>) {
    for candidate in Level::ALL {
        let key = format!("{metric}_{}", candidate.suffix());
        match status {
            Some(status) if candidate == level => tokens.assign(&key, status),
            _ => tokens.drop_key(&key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_tokens, classify, is_valid_key, next, truncate_chars, Level, LevelTracker,
        Thresholds, TokenOptions, MAX_TOKEN_KEYS, MAX_TOKEN_VALUE_CHARS, PERCENT_MARGIN,
    };
    use crate::metrics::{CpuMode, LoadAverages, MemoryStatus, Sample};
    use crate::render::format::{load_text, mem_text};
    use crate::render::graph::{block_bar, sparkline, GraphStyle};

    /// The number of cells in the framed bar a token value opens with.
    ///
    /// The closing frame character doubles as the one-eighth block, so the bar
    /// ends at the last one in the value rather than the first; the reading
    /// that follows the bar never contains it.
    fn bar_cells(value: &str) -> usize {
        let inner = value
            .strip_prefix('\u{2595}')
            .unwrap_or_else(|| panic!("{value} does not open with a frame"));
        let end = inner
            .rfind('\u{258f}')
            .unwrap_or_else(|| panic!("{value} has no closing frame"));
        inner[..end].chars().count()
    }

    fn sample(cpu_percent: f32) -> Sample {
        Sample {
            cpu_percent,
            memory: MemoryStatus {
                used_bytes: 2885 * 1024 * 1024,
                total_bytes: 7987 * 1024 * 1024,
            },
            load: LoadAverages {
                one: 2.11,
                five: 2.35,
                fifteen: 2.44,
            },
            cpu_count: 8,
        }
    }

    #[test]
    fn default_thresholds_classify_cpu_load() {
        let thresholds = Thresholds::default();
        let level = |percent: f64| {
            classify(
                percent,
                f64::from(thresholds.cpu_warn),
                f64::from(thresholds.cpu_hot),
            )
        };
        assert_eq!(level(10.0), Level::Ok);
        assert_eq!(level(60.0), Level::Warn);
        assert_eq!(level(95.0), Level::Hot);
        // The boundaries belong to the higher level.
        assert_eq!(level(50.0), Level::Warn);
        assert_eq!(level(80.0), Level::Hot);
    }

    #[test]
    fn a_level_only_drops_once_the_value_clears_the_margin() {
        let (warn, hot) = (50.0, 80.0);
        let mut level = Level::Ok;

        // Escalation is immediate.
        level = next(level, 79.0, warn, hot, PERCENT_MARGIN);
        assert_eq!(level, Level::Warn);
        level = next(level, 81.0, warn, hot, PERCENT_MARGIN);
        assert_eq!(level, Level::Hot);

        // Oscillating around the threshold keeps it there.
        for value in [79.0, 81.0, 79.0, 75.0] {
            level = next(level, value, warn, hot, PERCENT_MARGIN);
            assert_eq!(level, Level::Hot, "{value} is inside the margin");
        }

        // Only a value a full margin below lets it fall back.
        level = next(level, 74.9, warn, hot, PERCENT_MARGIN);
        assert_eq!(level, Level::Warn);
        // And the same rule applies on the way from warn to ok.
        for value in [49.0, 45.0] {
            level = next(level, value, warn, hot, PERCENT_MARGIN);
            assert_eq!(level, Level::Warn, "{value} is inside the margin");
        }
        level = next(level, 44.9, warn, hot, PERCENT_MARGIN);
        assert_eq!(level, Level::Ok);
    }

    #[test]
    fn the_first_sample_is_plainly_classified() {
        for value in [0.0, 44.9, 49.0, 50.0, 74.9, 79.0, 80.0, 100.0] {
            assert_eq!(
                next(Level::Ok, value, 50.0, 80.0, PERCENT_MARGIN),
                classify(value, 50.0, 80.0),
                "a fresh tracker must agree with classify at {value}"
            );
        }
    }

    #[test]
    fn the_tracker_remembers_each_metric_separately() {
        let mut levels = LevelTracker::new();
        // 95% CPU is hot; 36% memory and a load of 2.11 over 8 cores are not.
        let hot = build_tokens(&sample(95.0), &[], &TokenOptions::default(), &mut levels);
        assert!(hot.value("cpu_hot").is_some());

        // 79% is below cpu_hot but inside the margin, so the CPU stays hot
        // while memory and load are untouched.
        let held = build_tokens(&sample(79.0), &[], &TokenOptions::default(), &mut levels);
        assert!(held.value("cpu_hot").is_some());
        assert!(held.value("mem_ok").is_some());
        assert!(held.value("load_ok").is_some());

        let dropped = build_tokens(&sample(74.0), &[], &TokenOptions::default(), &mut levels);
        assert!(dropped.value("cpu_hot").is_none());
        assert!(dropped.value("cpu_warn").is_some());
    }

    #[test]
    fn the_memory_bar_can_be_narrower_than_the_cpu_bar() {
        let opts = TokenOptions {
            graph_lines: 10,
            mem_graph_lines: 4,
            ..TokenOptions::default()
        };
        let tokens = build_tokens(&sample(51.2), &[], &opts, &mut LevelTracker::new());

        let mem = tokens.value("mem_status").expect("mem_status is set");
        let cpu = tokens.value("cpu_status").expect("cpu_status is set");
        // 36.1% of four cells is one full cell plus a half, inside the frame.
        assert!(
            mem.starts_with("\u{2595}\u{2588}\u{258c}  \u{258f} "),
            "{mem}"
        );
        // The CPU bar still gets all ten.
        assert!(
            cpu.starts_with(
                "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258f}    \u{258f} "
            ),
            "{cpu}"
        );
        // The level token mirrors the same string.
        assert_eq!(tokens.value("mem_ok"), Some(mem));
    }

    #[test]
    fn the_load_bar_can_be_narrower_than_the_cpu_bar() {
        let opts = TokenOptions {
            graph_lines: 10,
            load_graph_lines: 4,
            ..TokenOptions::default()
        };
        let tokens = build_tokens(&sample(51.2), &[], &opts, &mut LevelTracker::new());

        let load = tokens.value("load_status").expect("load_status is set");
        let cpu = tokens.value("cpu_status").expect("cpu_status is set");
        // A load of 2.11 over eight cores is 26.4% of a saturated machine,
        // drawn across four cells rather than the ten the CPU gets.
        assert_eq!(load, format!("{} 2.11 2.35 2.44", block_bar(26.375, 4)));
        assert_eq!(bar_cells(load), 4, "{load}");
        // The CPU bar still gets all ten, and memory follows it.
        assert!(
            cpu.starts_with(
                "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258f}    \u{258f} "
            ),
            "{cpu}"
        );
        let mem = tokens.value("mem_status").expect("mem_status is set");
        assert_eq!(bar_cells(mem), 10, "{mem}");
        // The level token mirrors the same string.
        assert_eq!(tokens.value("load_ok"), Some(load));
    }

    #[test]
    fn the_three_bars_are_independent() {
        let opts = TokenOptions {
            graph_lines: 10,
            mem_graph_lines: 4,
            load_graph_lines: 6,
            ..TokenOptions::default()
        };
        let tokens = build_tokens(&sample(51.2), &[], &opts, &mut LevelTracker::new());

        for (key, cells) in [("cpu_status", 10), ("mem_status", 4), ("load_status", 6)] {
            let value = tokens.value(key).unwrap_or_else(|| panic!("{key} is set"));
            assert_eq!(bar_cells(value), cells, "{key} = {value}");
        }
    }

    #[test]
    fn build_tokens_sets_every_status_key() {
        let tokens = build_tokens(
            &sample(51.2),
            &[],
            &TokenOptions::default(),
            &mut LevelTracker::new(),
        );

        assert_eq!(
            tokens.value("cpu_status"),
            Some("\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258f}    \u{258f} 51.2%")
        );
        assert_eq!(
            tokens.value("mem_status"),
            Some("\u{2595}\u{2588}\u{2588}\u{2588}\u{258b}      \u{258f} 2885/7987MB")
        );
        assert_eq!(
            tokens.value("load_status"),
            // The load bar defaults to four cells, not the CPU graph's ten.
            Some("\u{2595}\u{2588}   \u{258f} 2.11 2.35 2.44")
        );
        assert!(tokens
            .value("sys_status")
            .expect("sys_status is set")
            .contains("2885/7987MB"));
    }

    #[test]
    fn exactly_one_level_token_is_set_per_metric() {
        // 95% CPU is hot, 36% memory is ok, load 2.11 over 8 cores is ok.
        let tokens = build_tokens(
            &sample(95.0),
            &[],
            &TokenOptions::default(),
            &mut LevelTracker::new(),
        );

        for (metric, expected) in [("cpu", Level::Hot), ("mem", Level::Ok), ("load", Level::Ok)] {
            let status = tokens
                .value(&format!("{metric}_status"))
                .expect("the metric has a status")
                .to_string();
            for level in Level::ALL {
                let key = format!("{metric}_{}", level.suffix());
                if level == expected {
                    assert_eq!(tokens.value(&key), Some(status.as_str()), "{key}");
                    assert!(!tokens.clears(&key), "{key} must not also be cleared");
                } else {
                    assert_eq!(tokens.value(&key), None, "{key}");
                    assert!(tokens.clears(&key), "{key} must be cleared");
                }
            }
        }
    }

    #[test]
    fn history_drives_the_sparkline_token() {
        let empty = build_tokens(
            &sample(51.2),
            &[],
            &TokenOptions::default(),
            &mut LevelTracker::new(),
        );
        assert_eq!(empty.value("cpu_history"), None);
        assert!(empty.clears("cpu_history"));

        let history = [0.0, 50.0, 100.0];
        let filled = build_tokens(
            &sample(51.2),
            &history,
            &TokenOptions::default(),
            &mut LevelTracker::new(),
        );
        assert_eq!(
            filled.value("cpu_history"),
            Some(sparkline(&history).as_str())
        );
        assert!(!filled.clears("cpu_history"));
    }

    #[test]
    fn zero_averages_clears_the_load_tokens() {
        let opts = TokenOptions {
            averages_count: 0,
            ..TokenOptions::default()
        };
        let tokens = build_tokens(&sample(51.2), &[], &opts, &mut LevelTracker::new());

        assert_eq!(tokens.value("load_status"), None);
        assert!(tokens.clears("load_status"));
        for level in Level::ALL {
            assert!(tokens.clears(&format!("load_{}", level.suffix())));
        }
        // CPU and memory are untouched by the load setting.
        assert!(tokens.value("cpu_status").is_some());
        assert!(tokens.value("mem_status").is_some());
    }

    #[test]
    fn every_report_fits_inside_the_herdr_limits() {
        let history: Vec<f32> = (0u8..64).map(|step| f32::from(step) * 1.5).collect();
        for averages_count in [0, 3] {
            for graph_lines in [0, 10, 40] {
                let opts = TokenOptions {
                    graph_lines,
                    mem_graph_lines: graph_lines,
                    load_graph_lines: graph_lines,
                    averages_count,
                    ..TokenOptions::default()
                };
                let tokens = build_tokens(&sample(99.9), &history, &opts, &mut LevelTracker::new());

                tokens.validate().expect("report is within herdr's limits");
                assert!(
                    tokens.mentioned_keys() <= MAX_TOKEN_KEYS,
                    "{} keys",
                    tokens.mentioned_keys()
                );
                // The design mentions exactly 14 keys: five status keys plus
                // three levels for each of the three metrics.
                assert_eq!(tokens.mentioned_keys(), 14);
                for (key, value) in &tokens.set {
                    assert!(is_valid_key(key), "{key}");
                    assert!(
                        value.chars().count() <= MAX_TOKEN_VALUE_CHARS,
                        "{key} is {} characters",
                        value.chars().count()
                    );
                    assert!(!value.chars().any(char::is_control), "{key}");
                }
            }
        }
    }

    /// Every field as wide as it realistically gets: four terabytes of memory,
    /// a load average in the thousands, and enough threads that
    /// [`CpuMode::Threads`] prints five digits.
    fn extreme_sample() -> Sample {
        Sample {
            cpu_percent: 99.9,
            memory: MemoryStatus {
                used_bytes: 9_999 * 1024 * 1024,
                total_bytes: 4_000_000 * 1024 * 1024,
            },
            load: LoadAverages {
                one: 9_999.99,
                five: 8_888.88,
                fifteen: 7_777.77,
            },
            cpu_count: 256,
        }
    }

    #[test]
    fn a_wide_bar_is_narrowed_rather_than_the_reading_truncated() {
        let history: Vec<f32> = (0u8..64).map(|step| f32::from(step) * 1.5).collect();

        for graph_lines in [0, 10, 40, 60, 64] {
            for graph_style in [
                GraphStyle::Classic,
                GraphStyle::Blocks,
                GraphStyle::Vertical,
            ] {
                for cpu_mode in [CpuMode::Default, CpuMode::Threads] {
                    for sample in [sample(51.2), extreme_sample()] {
                        let opts = TokenOptions {
                            graph_lines,
                            mem_graph_lines: graph_lines,
                            load_graph_lines: graph_lines,
                            graph_style,
                            cpu_mode,
                            ..TokenOptions::default()
                        };
                        let tokens =
                            build_tokens(&sample, &history, &opts, &mut LevelTracker::new());
                        let context = format!("{graph_lines} cells, {graph_style}, {cpu_mode:?}");

                        tokens
                            .validate()
                            .unwrap_or_else(|error| panic!("{context}: {error}"));

                        // Nothing was cut: the reading is what sits at the end
                        // of a value, and the bar is what gives way.
                        let memory = mem_text(&sample.memory, opts.mem_mode);
                        let load = load_text(&sample.load, opts.averages_count);
                        for key in ["mem_status", "mem_ok", "mem_warn", "mem_hot"] {
                            if let Some(value) = tokens.value(key) {
                                assert!(value.ends_with(&memory), "{context}: {key} = {value}");
                            }
                        }
                        for key in ["load_status", "load_ok", "load_warn", "load_hot"] {
                            if let Some(value) = tokens.value(key) {
                                assert!(value.ends_with(load.trim()), "{context}: {key} = {value}");
                            }
                        }
                        let sys = tokens.value("sys_status").expect("sys_status is set");
                        assert!(sys.starts_with(&memory), "{context}: {sys}");
                        assert!(sys.ends_with(&load), "{context}: {sys}");
                    }
                }
            }
        }
    }

    #[test]
    fn truncate_chars_cuts_on_character_boundaries() {
        let blocks = "\u{2588}".repeat(100);
        let cut = truncate_chars(&blocks, MAX_TOKEN_VALUE_CHARS);
        assert_eq!(cut.chars().count(), MAX_TOKEN_VALUE_CHARS);
        assert_eq!(cut.len(), MAX_TOKEN_VALUE_CHARS * 3);
        assert!(cut.chars().all(|ch| ch == '\u{2588}'));

        assert_eq!(truncate_chars("short", 80), "short");
        assert_eq!(truncate_chars("short", 0), "");
    }

    #[test]
    fn token_keys_follow_the_herdr_key_rule() {
        assert!(is_valid_key("cpu_status"));
        assert!(is_valid_key("cpu-history"));
        assert!(!is_valid_key(""));
        assert!(!is_valid_key("cpu status"));
        assert!(!is_valid_key("cpu.status"));
        assert!(!is_valid_key(&"c".repeat(33)));
    }
}
