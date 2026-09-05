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
use crate::render::format::{cpu_text, load_text, mem_text, status_line, RenderOptions};
use crate::render::graph::{render_bar, sparkline, GraphStyle};

/// herdr accepts at most 16 token keys in one metadata report.
pub const MAX_TOKEN_KEYS: usize = 16;
/// herdr caps a token value at 80 characters (`char`s, not bytes).
pub const MAX_TOKEN_VALUE_CHARS: usize = 80;
/// herdr caps a token key at 32 characters.
pub const MAX_TOKEN_KEY_CHARS: usize = 32;

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

/// Everything the token builder needs beyond the sample.
///
/// The defaults are the daemon's, which differ from one-line mode in one place:
/// the sidebar gets the unicode [`GraphStyle::Blocks`] bar rather than the
/// ASCII bar the original `tmux-mem-cpu-load` prints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenOptions {
    pub graph_style: GraphStyle,
    pub graph_lines: usize,
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
            mem_mode: MemoryMode::Default,
            cpu_mode: CpuMode::Default,
            averages_count: 3,
            thresholds: Thresholds::default(),
        }
    }
}

impl TokenOptions {
    /// The one-line renderer's options, used for the combined `sys_status`.
    #[must_use]
    pub const fn render_options(&self) -> RenderOptions {
        RenderOptions {
            mem_mode: self.mem_mode,
            cpu_mode: self.cpu_mode,
            graph_style: self.graph_style,
            graph_lines: self.graph_lines,
            averages_count: self.averages_count,
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
/// `cpu_history` sparkline; an empty history clears that token.
#[must_use]
pub fn build_tokens(sample: &Sample, history: &[f32], opts: &TokenOptions) -> TokenSet {
    let mut tokens = TokenSet::default();

    let cpu_status = format!(
        "{} {}",
        render_bar(opts.graph_style, sample.cpu_percent, opts.graph_lines),
        cpu_text(sample.cpu_percent, opts.cpu_mode, sample.cpu_count).trim()
    );
    tokens.assign("cpu_status", &cpu_status);

    let mem_percent = sample.memory.used_percent();
    let mem_status = format!(
        "{} {}",
        render_bar(opts.graph_style, mem_percent, opts.graph_lines),
        mem_text(&sample.memory, opts.mem_mode)
    );
    tokens.assign("mem_status", &mem_status);

    let load = load_per_core(&sample.load, sample.cpu_count);
    let load_status = (opts.averages_count > 0).then(|| {
        format!(
            "{} {}",
            render_bar(
                opts.graph_style,
                (load * 100.0).min(100.0) as f32,
                opts.graph_lines
            ),
            load_text(&sample.load, opts.averages_count).trim()
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

    let levels = [
        classify(
            f64::from(sample.cpu_percent),
            f64::from(opts.thresholds.cpu_warn),
            f64::from(opts.thresholds.cpu_hot),
        ),
        classify(
            f64::from(mem_percent),
            f64::from(opts.thresholds.mem_warn),
            f64::from(opts.thresholds.mem_hot),
        ),
        classify(load, opts.thresholds.load_warn, opts.thresholds.load_hot),
    ];
    let statuses = [Some(cpu_status), Some(mem_status), load_status];
    for ((metric, level), status) in METRICS.iter().zip(levels).zip(&statuses) {
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
        build_tokens, classify, is_valid_key, truncate_chars, Level, Thresholds, TokenOptions,
        MAX_TOKEN_KEYS, MAX_TOKEN_VALUE_CHARS,
    };
    use crate::metrics::{LoadAverages, MemoryStatus, Sample};
    use crate::render::graph::sparkline;

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
    fn build_tokens_sets_every_status_key() {
        let tokens = build_tokens(&sample(51.2), &[], &TokenOptions::default());

        assert_eq!(
            tokens.value("cpu_status"),
            Some("\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258f}    \u{258f} 51.2%")
        );
        assert!(tokens
            .value("mem_status")
            .expect("mem_status is set")
            .ends_with(" 2885/7987MB"));
        assert_eq!(
            tokens.value("load_status"),
            Some("\u{2595}\u{2588}\u{2588}\u{258b}       \u{258f} 2.11 2.35 2.44")
        );
        assert!(tokens
            .value("sys_status")
            .expect("sys_status is set")
            .contains("2885/7987MB"));
    }

    #[test]
    fn exactly_one_level_token_is_set_per_metric() {
        // 95% CPU is hot, 36% memory is ok, load 2.11 over 8 cores is ok.
        let tokens = build_tokens(&sample(95.0), &[], &TokenOptions::default());

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
        let empty = build_tokens(&sample(51.2), &[], &TokenOptions::default());
        assert_eq!(empty.value("cpu_history"), None);
        assert!(empty.clears("cpu_history"));

        let history = [0.0, 50.0, 100.0];
        let filled = build_tokens(&sample(51.2), &history, &TokenOptions::default());
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
        let tokens = build_tokens(&sample(51.2), &[], &opts);

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
                    averages_count,
                    ..TokenOptions::default()
                };
                let tokens = build_tokens(&sample(99.9), &history, &opts);

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
