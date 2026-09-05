//! Text formatting for the status line, ported from the original's
//! `cpu_string`, `mem_string`, and `load_string`.
//!
//! Without colours the output is byte for byte what the uncoloured original
//! prints. With `--colors` the tmux markup and powerline separators follow the
//! same control flow as `common/main.cc`, `common/memory.cc`, and
//! `common/load.cc`; with `--ansi` the same lookup tables are emitted as SGR
//! escapes instead.

use crate::metrics::load::load_percent;
use crate::metrics::memory::{MemoryMode, MemoryStatus};
use crate::metrics::{CpuMode, LoadAverages, Sample};
use crate::render::colors::{
    cpu_color, load_color, mem_color, powerline, powerline_char, ColorMode, PowerlineMode,
    ANSI_RESET, TMUX_RESET,
};
use crate::render::graph::{block_bar, classic_bar, vertical_bar, GraphStyle};

/// Everything the renderer needs to know beyond the sample itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderOptions {
    pub mem_mode: MemoryMode,
    pub cpu_mode: CpuMode,
    pub graph_style: GraphStyle,
    pub graph_lines: usize,
    pub averages_count: u8,
    /// Which colour markup to wrap each segment in.
    pub color: ColorMode,
    /// Which powerline separators to blend the tmux markup with.
    pub powerline: PowerlineMode,
    /// The tmux colour of whatever sits to the left of the status, for
    /// seamless powerline blending (`-l`).
    pub segments_left: Option<u16>,
    /// The tmux colour of whatever sits to the right of the status (`-r`).
    pub segments_right: Option<u16>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            mem_mode: MemoryMode::Default,
            cpu_mode: CpuMode::Default,
            graph_style: GraphStyle::Classic,
            graph_lines: 10,
            averages_count: 3,
            color: ColorMode::None,
            powerline: PowerlineMode::None,
            segments_left: None,
            segments_right: None,
        }
    }
}

/// The right-aligned CPU percentage, e.g. `  51.2%`.
///
/// Values of 100% and above drop the decimal to keep the field six wide.
#[must_use]
pub fn cpu_text(percent: f32, mode: CpuMode, cpu_count: u32) -> String {
    let multiplier = match mode {
        CpuMode::Default => 1.0,
        CpuMode::Threads => cpu_count as f32,
    };
    let value = percent * multiplier;
    if value >= 100.0 {
        format!("{value:>6.0}%")
    } else {
        format!("{value:>6.1}%")
    }
}

/// The CPU graph followed by the percentage, e.g. ` [|||||     ]  51.2%`.
#[must_use]
pub fn cpu_line(
    percent: f32,
    mode: CpuMode,
    cpu_count: u32,
    style: GraphStyle,
    graph_lines: usize,
) -> String {
    let text = cpu_text(percent, mode, cpu_count);
    match style {
        GraphStyle::Classic => {
            if graph_lines > 0 {
                format!(" {}{text}", classic_bar(percent, graph_lines))
            } else {
                text
            }
        }
        GraphStyle::Blocks => format!("{}{text}", block_bar(percent, graph_lines)),
        GraphStyle::Vertical => format!("{}{text}", vertical_bar(percent)),
    }
}

/// Memory usage in the requested mode, working in megabytes like the original.
#[must_use]
pub fn mem_text(status: &MemoryStatus, mode: MemoryMode) -> String {
    let used = status.used_mb();
    let total = status.total_mb();

    match mode {
        MemoryMode::Free => {
            let free = total - used;
            let free_gb = free / 1024.0;
            if free_gb < 1.0 {
                format!("{free:.2}MB")
            } else {
                format!("{free_gb:.2}GB")
            }
        }
        MemoryMode::UsagePercent => format!("{:.2}%", status.used_percent()),
        MemoryMode::Default => {
            if used > 10_000.0 && total > 10_000.0 {
                format!("{}/{}GB", (used / 1024.0) as u32, (total / 1024.0) as u32)
            } else if used < 10_000.0 && total > 10_000.0 {
                format!("{}MB/{}GB", used as u32, (total / 1024.0) as u32)
            } else {
                format!("{}/{}MB", used as u32, total as u32)
            }
        }
    }
}

/// The load averages, e.g. ` 2.11 2.35 2.44`. Empty when `averages_count` is 0.
#[must_use]
pub fn load_text(load: &LoadAverages, averages_count: u8) -> String {
    let all = [load.one, load.five, load.fifteen];
    let count = (averages_count as usize).min(all.len());
    if count == 0 {
        return String::new();
    }

    let rendered: Vec<String> = all[..count]
        .iter()
        .map(|value| {
            // Round to nearest hundredth the way the original's floorf() does,
            // so 2.345 prints as 2.35 rather than 2.34.
            let rounded = (value * 100.0 + 0.5).floor() / 100.0;
            format!("{rounded:.2}")
        })
        .collect();

    format!(" {}", rendered.join(" "))
}

/// The memory segment, coloured the way `mem_string` colours it.
#[must_use]
pub fn mem_segment(status: &MemoryStatus, opts: &RenderOptions) -> String {
    let text = mem_text(status, opts.mem_mode);
    let percent = status.used_percent() as u32;

    match opts.color {
        ColorMode::None => text,
        ColorMode::Ansi => format!("{}{text}{ANSI_RESET}", mem_color(percent).ansi()),
        ColorMode::Tmux => {
            let color = mem_color(percent).tmux();
            // The memory segment starts the line, so it is the one that has to
            // blend with whatever tmux drew to its left.
            let mut out = match (opts.powerline, opts.segments_left) {
                (PowerlineMode::Right, Some(left)) => {
                    format!(
                        "{} ",
                        powerline_char(&color, left, PowerlineMode::Right, false)
                    )
                }
                (PowerlineMode::Right, None) => format!(
                    "#[bg=default]{} ",
                    powerline(&color, PowerlineMode::Right, false)
                ),
                (PowerlineMode::Left, Some(left)) => {
                    format!(
                        "{} ",
                        powerline_char(&color, left, PowerlineMode::Left, false)
                    )
                }
                // There is no way to invert the default background, so the
                // left-pointing separator is skipped at the start of the line.
                (PowerlineMode::Left, None) => {
                    format!("{} ", powerline(&color, PowerlineMode::None, false))
                }
                (PowerlineMode::None, _) => powerline(&color, PowerlineMode::None, false),
            };
            out.push_str(&text);
            out.push_str(&tmux_segment_end(&color, opts.powerline));
            out
        }
    }
}

/// The CPU segment, coloured the way `cpu_string` colours it.
#[must_use]
pub fn cpu_segment(sample: &Sample, opts: &RenderOptions) -> String {
    let text = cpu_line(
        sample.cpu_percent,
        opts.cpu_mode,
        sample.cpu_count,
        opts.graph_style,
        opts.graph_lines,
    );
    // The lookup index is the raw percentage, not the one scaled by the thread
    // count that gets printed.
    let percent = truncate_percent(sample.cpu_percent);

    match opts.color {
        ColorMode::None => text,
        ColorMode::Ansi => format!("{}{text}{ANSI_RESET}", cpu_color(percent).ansi()),
        ColorMode::Tmux => {
            let color = cpu_color(percent).tmux();
            format!(
                "{}{text}{}",
                powerline(&color, opts.powerline, false),
                tmux_segment_end(&color, opts.powerline)
            )
        }
    }
}

/// The load segment, coloured the way `load_string` colours it.
#[must_use]
pub fn load_segment(sample: &Sample, opts: &RenderOptions) -> String {
    let text = load_text(&sample.load, opts.averages_count);
    if text.is_empty() {
        return text;
    }
    let percent = load_percent(&sample.load, sample.cpu_count);

    match opts.color {
        ColorMode::None => text,
        ColorMode::Ansi => format!("{}{text}{ANSI_RESET}", load_color(percent).ansi()),
        ColorMode::Tmux => {
            let color = load_color(percent).tmux();
            let mut out = powerline(&color, opts.powerline, false);
            out.push_str(&text);
            // The load segment ends the line, so it is the one that has to
            // blend with whatever tmux draws to its right.
            match (opts.powerline, opts.segments_right) {
                (PowerlineMode::Left, Some(right)) => {
                    out.push_str(&powerline(&color, PowerlineMode::Left, true));
                    out.push_str(&powerline_char(&color, right, PowerlineMode::Left, true));
                }
                (PowerlineMode::Left, None) => {
                    out.push_str(&powerline(&color, PowerlineMode::Left, true));
                    out.push_str(&powerline(TMUX_RESET, PowerlineMode::Left, false));
                }
                (PowerlineMode::Right, Some(right)) => {
                    out.push_str(&powerline_char(&color, right, PowerlineMode::Right, true));
                }
                (PowerlineMode::Right, None) => {}
                (PowerlineMode::None, _) => out.push_str(TMUX_RESET),
            }
            out
        }
    }
}

/// What `cpu_string` and `mem_string` append after their text: the
/// left-pointing separator's background flip, or a plain reset.
fn tmux_segment_end(color: &str, mode: PowerlineMode) -> String {
    match mode {
        PowerlineMode::Left => powerline(color, PowerlineMode::Left, true),
        PowerlineMode::Right => String::new(),
        PowerlineMode::None => TMUX_RESET.to_string(),
    }
}

/// The lookup table index for a percentage: truncated, and never negative.
fn truncate_percent(percent: f32) -> u32 {
    if percent <= 0.0 {
        0
    } else {
        percent as u32
    }
}

/// The complete one-line status: memory, then CPU, then load.
#[must_use]
pub fn status_line(sample: &Sample, opts: &RenderOptions) -> String {
    format!(
        "{}{}{}",
        mem_segment(&sample.memory, opts),
        cpu_segment(sample, opts),
        load_segment(sample, opts),
    )
}

#[cfg(test)]
mod tests {
    use super::{cpu_line, cpu_text, load_text, mem_text, status_line, RenderOptions};
    use crate::metrics::memory::{MemoryMode, MemoryStatus};
    use crate::metrics::{CpuMode, LoadAverages, Sample};
    use crate::render::colors::{ColorMode, PowerlineMode};
    use crate::render::graph::GraphStyle;

    const MB: u64 = 1024 * 1024;

    fn megabytes(used: u64, total: u64) -> MemoryStatus {
        MemoryStatus {
            used_bytes: used * MB,
            total_bytes: total * MB,
        }
    }

    fn sample() -> Sample {
        Sample {
            cpu_percent: 51.2,
            memory: megabytes(2885, 7987),
            load: LoadAverages {
                one: 2.11,
                five: 2.35,
                fifteen: 2.44,
            },
            cpu_count: 8,
        }
    }

    /// The lookup table entries the sample above lands on, taken from
    /// `common/luts.h`: 51% CPU, 36% memory, and a load percentage of
    /// `2.11 / 8 * 0.5 * 100` truncated to 13.
    const CPU_COLOR: &str = "#[fg=brightwhite,bg=colour56]";
    const MEM_COLOR: &str = "#[fg=brightwhite,bg=colour72]";
    const LOAD_COLOR: &str = "#[fg=brightwhite,bg=colour59]";
    const RESET: &str = "#[fg=default,bg=default]";

    #[test]
    fn cpu_text_is_right_aligned_in_six_columns() {
        assert_eq!(cpu_text(51.2, CpuMode::Default, 8), "  51.2%");
        assert_eq!(cpu_text(0.0, CpuMode::Default, 8), "   0.0%");
        assert_eq!(cpu_text(100.0, CpuMode::Default, 8), "   100%");
    }

    #[test]
    fn threads_mode_scales_by_the_cpu_count() {
        assert_eq!(cpu_text(51.2, CpuMode::Threads, 8), "   410%");
        assert_eq!(cpu_text(5.0, CpuMode::Threads, 8), "  40.0%");
    }

    #[test]
    fn cpu_line_renders_each_graph_style() {
        assert_eq!(
            cpu_line(51.2, CpuMode::Default, 8, GraphStyle::Classic, 10),
            " [|||||     ]  51.2%"
        );
        assert_eq!(
            cpu_line(51.2, CpuMode::Default, 8, GraphStyle::Classic, 0),
            "  51.2%"
        );
        assert_eq!(
            cpu_line(50.0, CpuMode::Default, 8, GraphStyle::Blocks, 10),
            "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}     \u{258f}  50.0%"
        );
        assert_eq!(
            cpu_line(51.2, CpuMode::Default, 8, GraphStyle::Vertical, 10),
            "\u{2595}\u{2585}\u{258f}  51.2%"
        );
    }

    #[test]
    fn mem_text_default_mode_picks_a_unit() {
        assert_eq!(
            mem_text(&megabytes(2885, 7987), MemoryMode::Default),
            "2885/7987MB"
        );
        assert_eq!(
            mem_text(&megabytes(11_156, 16_003), MemoryMode::Default),
            "10/15GB"
        );
        assert_eq!(
            mem_text(&megabytes(900, 16_003), MemoryMode::Default),
            "900MB/15GB"
        );
    }

    #[test]
    fn mem_text_free_mode_switches_between_mb_and_gb() {
        // 7987 - 2885 = 5102 MB, which is 4.98 GB.
        assert_eq!(mem_text(&megabytes(2885, 7987), MemoryMode::Free), "4.98GB");
        // 8000 - 7500 = 500 MB, under a gigabyte.
        assert_eq!(
            mem_text(&megabytes(7500, 8000), MemoryMode::Free),
            "500.00MB"
        );
    }

    #[test]
    fn mem_text_usage_percent_mode() {
        assert_eq!(
            mem_text(&megabytes(2885, 7987), MemoryMode::UsagePercent),
            "36.12%"
        );
        assert_eq!(
            mem_text(&megabytes(0, 0), MemoryMode::UsagePercent),
            "0.00%"
        );
    }

    #[test]
    fn load_text_honours_the_averages_count() {
        let load = LoadAverages {
            one: 2.11,
            five: 2.35,
            fifteen: 2.44,
        };
        assert_eq!(load_text(&load, 0), "");
        assert_eq!(load_text(&load, 1), " 2.11");
        assert_eq!(load_text(&load, 3), " 2.11 2.35 2.44");
    }

    #[test]
    fn load_text_rounds_to_the_nearest_hundredth() {
        let load = LoadAverages {
            one: 2.345,
            five: 0.0,
            fifteen: 0.0,
        };
        assert_eq!(load_text(&load, 1), " 2.35");
    }

    #[test]
    fn status_line_matches_the_original_example() {
        assert_eq!(
            status_line(&sample(), &RenderOptions::default()),
            "2885/7987MB [|||||     ]  51.2% 2.11 2.35 2.44"
        );
    }

    fn colored(color: ColorMode, powerline: PowerlineMode) -> RenderOptions {
        RenderOptions {
            color,
            powerline,
            ..RenderOptions::default()
        }
    }

    #[test]
    fn tmux_colors_wrap_every_segment_like_the_original() {
        assert_eq!(
            status_line(&sample(), &colored(ColorMode::Tmux, PowerlineMode::None)),
            format!(
                "{MEM_COLOR}2885/7987MB{RESET}\
                 {CPU_COLOR} [|||||     ]  51.2%{RESET}\
                 {LOAD_COLOR} 2.11 2.35 2.44{RESET}"
            )
        );
    }

    #[test]
    fn ansi_colors_use_the_same_tables() {
        // colour72 with a brightwhite (colour15) foreground.
        let line = status_line(&sample(), &colored(ColorMode::Ansi, PowerlineMode::None));
        assert!(
            line.starts_with("\x1b[38;5;15m\x1b[48;5;72m2885/7987MB\x1b[0m"),
            "{line}"
        );
        assert!(
            line.contains("\x1b[48;5;56m [|||||     ]  51.2%\x1b[0m"),
            "{line}"
        );
        assert!(
            line.ends_with("\x1b[48;5;59m 2.11 2.35 2.44\x1b[0m"),
            "{line}"
        );
    }

    #[test]
    fn powerline_left_inverts_each_segment_background() {
        let line = status_line(&sample(), &colored(ColorMode::Tmux, PowerlineMode::Left));
        assert_eq!(
            line,
            format!(
                "{MEM_COLOR} 2885/7987MB #[fg=colour72]\
                 #[bg=colour56]\u{e0b0}{CPU_COLOR} [|||||     ]  51.2% #[fg=colour56]\
                 #[bg=colour59]\u{e0b0}{LOAD_COLOR} 2.11 2.35 2.44 #[fg=colour59]\
                 #[bg=default]\u{e0b0}{RESET}"
            )
        );
    }

    #[test]
    fn powerline_right_points_the_separators_the_other_way() {
        let line = status_line(&sample(), &colored(ColorMode::Tmux, PowerlineMode::Right));
        assert_eq!(
            line,
            format!(
                "#[bg=default] #[fg=colour72]\u{e0b2}{MEM_COLOR} 2885/7987MB \
                 #[fg=colour56]\u{e0b2}{CPU_COLOR} [|||||     ]  51.2% \
                 #[fg=colour59]\u{e0b2}{LOAD_COLOR} 2.11 2.35 2.44"
            )
        );
    }

    #[test]
    fn segment_colors_blend_with_the_neighbouring_tmux_colours() {
        let opts = RenderOptions {
            segments_left: Some(4),
            segments_right: Some(8),
            ..colored(ColorMode::Tmux, PowerlineMode::Right)
        };
        let line = status_line(&sample(), &opts);
        assert!(
            line.starts_with(&format!("#[fg=colour72] #[bg=colour4]\u{e0b2}{MEM_COLOR} ")),
            "{line}"
        );
        assert!(
            line.ends_with(&format!("{LOAD_COLOR}#[fg=colour8] \u{e0b2}{LOAD_COLOR}")),
            "{line}"
        );
    }

    #[test]
    fn colours_never_leak_into_the_plain_output() {
        let plain = status_line(&sample(), &RenderOptions::default());
        assert!(!plain.contains("#["));
        assert!(!plain.contains('\x1b'));
    }

    #[test]
    fn zero_averages_leaves_the_load_segment_out_even_in_colour() {
        let opts = RenderOptions {
            averages_count: 0,
            ..colored(ColorMode::Tmux, PowerlineMode::None)
        };
        let line = status_line(&sample(), &opts);
        assert!(line.ends_with(&format!("  51.2%{RESET}")), "{line}");
        assert!(!line.contains(LOAD_COLOR), "{line}");
    }
}
