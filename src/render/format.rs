//! Text formatting for the status line, ported from the original's
//! `cpu_string`, `mem_string`, and `load_string` without the tmux colour
//! markup (colours are reserved for a later phase).

use crate::metrics::memory::{MemoryMode, MemoryStatus};
use crate::metrics::{CpuMode, LoadAverages, Sample};
use crate::render::graph::{block_bar, classic_bar, vertical_bar, GraphStyle};

/// Everything the renderer needs to know beyond the sample itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderOptions {
    pub mem_mode: MemoryMode,
    pub cpu_mode: CpuMode,
    pub graph_style: GraphStyle,
    pub graph_lines: usize,
    pub averages_count: u8,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            mem_mode: MemoryMode::Default,
            cpu_mode: CpuMode::Default,
            graph_style: GraphStyle::Classic,
            graph_lines: 10,
            averages_count: 3,
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

/// The complete one-line status: memory, then CPU, then load.
#[must_use]
pub fn status_line(sample: &Sample, opts: &RenderOptions) -> String {
    format!(
        "{}{}{}",
        mem_text(&sample.memory, opts.mem_mode),
        cpu_line(
            sample.cpu_percent,
            opts.cpu_mode,
            sample.cpu_count,
            opts.graph_style,
            opts.graph_lines,
        ),
        load_text(&sample.load, opts.averages_count),
    )
}

#[cfg(test)]
mod tests {
    use super::{cpu_line, cpu_text, load_text, mem_text, status_line, RenderOptions};
    use crate::metrics::memory::{MemoryMode, MemoryStatus};
    use crate::metrics::{CpuMode, LoadAverages, Sample};
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
}
