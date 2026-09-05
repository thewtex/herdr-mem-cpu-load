//! Bar graph rendering: the classic ASCII bar of `tmux-mem-cpu-load` plus the
//! unicode block styles herdr uses in the Space sidebar.

use std::fmt;
use std::str::FromStr;

use crate::sys::SysError;

/// Eighth-width block characters, from one eighth to seven eighths.
const PARTIAL_BLOCKS: [&str; 7] = [
    "\u{258f}", "\u{258e}", "\u{258d}", "\u{258c}", "\u{258b}", "\u{258a}", "\u{2589}",
];
/// Full block.
const FULL_BLOCK: &str = "\u{2588}";
/// Left and right frame characters for the unicode bars.
const FRAME_LEFT: &str = "\u{2595}";
const FRAME_RIGHT: &str = "\u{258f}";
/// Bottom-anchored blocks used by the vertical bar and the sparkline.
const RISING_BLOCKS: [&str; 8] = [
    "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2585}", "\u{2586}", "\u{2587}", "\u{2588}",
];

/// How the CPU graph is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphStyle {
    /// `[|||||     ]`, the original ASCII bar.
    #[default]
    Classic,
    /// `\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258c}    \u{258f}`, a framed eighth-resolution block bar.
    Blocks,
    /// A single vertical block character, for narrow status bars.
    Vertical,
}

impl FromStr for GraphStyle {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "classic" => Ok(Self::Classic),
            "blocks" => Ok(Self::Blocks),
            "vertical" => Ok(Self::Vertical),
            other => Err(format!(
                "invalid graph style `{other}`, expected classic, blocks, or vertical"
            )),
        }
    }
}

impl fmt::Display for GraphStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Classic => "classic",
            Self::Blocks => "blocks",
            Self::Vertical => "vertical",
        })
    }
}

impl TryFrom<&str> for GraphStyle {
    type Error = SysError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse().map_err(SysError::new)
    }
}

/// The original ASCII bar: `[|||||     ]` for 51.2% over 10 lines.
///
/// Dividing by 99.9 rather than 100 is deliberate: it is what the C++ original
/// does so that a hair under 100% still fills the bar.
#[must_use]
pub fn classic_bar(percent: f32, len: usize) -> String {
    let bar_count = (percent / 99.9 * len as f32) as usize;
    let mut bars = String::with_capacity(len + 2);
    bars.push('[');
    for _ in 0..bar_count {
        bars.push('|');
    }
    for _ in bar_count..len {
        bars.push(' ');
    }
    bars.push(']');
    bars
}

/// A framed horizontal bar at eighth-of-a-cell resolution:
/// `\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258c}    \u{258f}` for 55% over 10 cells.
#[must_use]
pub fn block_bar(percent: f32, len: usize) -> String {
    if len == 0 {
        return String::new();
    }

    let eighths = ((percent.clamp(0.0, 100.0) / 100.0) * (len * 8) as f32).round() as usize;
    let full = eighths / 8;
    let remainder = eighths % 8;

    let mut cells = String::with_capacity(len * 3 + 6);
    cells.push_str(FRAME_LEFT);
    for _ in 0..full {
        cells.push_str(FULL_BLOCK);
    }
    let mut drawn = full;
    if remainder > 0 && drawn < len {
        cells.push_str(PARTIAL_BLOCKS[remainder - 1]);
        drawn += 1;
    }
    for _ in drawn..len {
        cells.push(' ');
    }
    cells.push_str(FRAME_RIGHT);
    cells
}

/// The single character of the original vertical graph.
///
/// Picks the highest threshold that is at or below the truncated percentage.
#[must_use]
pub fn vertical_char(percent: f32) -> &'static str {
    const STEPS: [(u32, &str); 10] = [
        (0, " "),
        (10, RISING_BLOCKS[0]),
        (20, RISING_BLOCKS[1]),
        (30, RISING_BLOCKS[2]),
        (40, RISING_BLOCKS[3]),
        (50, RISING_BLOCKS[4]),
        (60, RISING_BLOCKS[5]),
        (70, RISING_BLOCKS[6]),
        (80, RISING_BLOCKS[7]),
        (90, "\u{25b2}"),
    ];

    let value = if percent <= 0.0 { 0 } else { percent as u32 };
    STEPS
        .iter()
        .rev()
        .find(|(threshold, _)| value >= *threshold)
        .map_or(" ", |(_, glyph)| *glyph)
}

/// [`vertical_char`] wrapped in the same frame the block bar uses.
#[must_use]
pub fn vertical_bar(percent: f32) -> String {
    format!("{FRAME_LEFT}{}{FRAME_RIGHT}", vertical_char(percent))
}

/// A sparkline over a history of 0-100 values, one character per value.
#[must_use]
pub fn sparkline(values: &[f32]) -> String {
    values
        .iter()
        .map(|value| {
            let bucket = (value.clamp(0.0, 100.0) / 100.0 * 8.0) as usize;
            RISING_BLOCKS[bucket.min(RISING_BLOCKS.len() - 1)]
        })
        .collect()
}

/// Render `percent` in the requested style. `len` is ignored by
/// [`GraphStyle::Vertical`], which is always one character wide.
#[must_use]
pub fn render_bar(style: GraphStyle, percent: f32, len: usize) -> String {
    match style {
        GraphStyle::Classic => classic_bar(percent, len),
        GraphStyle::Blocks => block_bar(percent, len),
        GraphStyle::Vertical => vertical_bar(percent),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        block_bar, classic_bar, render_bar, sparkline, vertical_bar, vertical_char, GraphStyle,
    };

    #[test]
    fn classic_bar_matches_the_original() {
        assert_eq!(classic_bar(51.2, 10), "[|||||     ]");
        assert_eq!(classic_bar(0.0, 10), "[          ]");
        assert_eq!(classic_bar(100.0, 10), "[||||||||||]");
        assert_eq!(classic_bar(51.2, 0), "[]");
    }

    #[test]
    fn block_bar_renders_eighths_inside_a_frame() {
        assert_eq!(block_bar(0.0, 10), "\u{2595}          \u{258f}");
        assert_eq!(
            block_bar(50.0, 10),
            "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}     \u{258f}"
        );
        // 55% of ten cells is 5 full cells plus a half cell.
        assert_eq!(
            block_bar(55.0, 10),
            "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258c}    \u{258f}"
        );
        // 56.25% of ten cells is 5 full cells plus five eighths.
        assert_eq!(
            block_bar(56.25, 10),
            "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258b}    \u{258f}"
        );
        assert_eq!(
            block_bar(100.0, 10),
            "\u{2595}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258f}"
        );
        assert_eq!(block_bar(50.0, 0), "");
    }

    #[test]
    fn block_bar_always_has_len_cells() {
        for percent in 0..=100 {
            let bar = block_bar(percent as f32, 10);
            assert_eq!(
                bar.chars().count(),
                12,
                "10 cells plus 2 frame characters at {percent}%"
            );
        }
    }

    #[test]
    fn vertical_char_matches_the_original_thresholds() {
        assert_eq!(vertical_char(0.0), " ");
        assert_eq!(vertical_char(15.0), "\u{2581}");
        assert_eq!(vertical_char(85.0), "\u{2588}");
        assert_eq!(vertical_char(95.0), "\u{25b2}");
        assert_eq!(vertical_bar(15.0), "\u{2595}\u{2581}\u{258f}");
    }

    #[test]
    fn sparkline_buckets_values_into_eight_blocks() {
        assert_eq!(sparkline(&[0.0, 50.0, 100.0]), "\u{2581}\u{2585}\u{2588}");
        assert_eq!(sparkline(&[]), "");
    }

    #[test]
    fn graph_style_parses_case_insensitively() {
        assert_eq!("classic".parse(), Ok(GraphStyle::Classic));
        assert_eq!("BLOCKS".parse(), Ok(GraphStyle::Blocks));
        assert_eq!("Vertical".parse(), Ok(GraphStyle::Vertical));
        assert!("spiral".parse::<GraphStyle>().is_err());
        assert_eq!(GraphStyle::Blocks.to_string(), "blocks");
        assert_eq!(GraphStyle::default(), GraphStyle::Classic);
    }

    #[test]
    fn render_bar_dispatches_by_style() {
        assert_eq!(
            render_bar(GraphStyle::Classic, 51.2, 10),
            classic_bar(51.2, 10)
        );
        assert_eq!(
            render_bar(GraphStyle::Blocks, 51.2, 10),
            block_bar(51.2, 10)
        );
        assert_eq!(
            render_bar(GraphStyle::Vertical, 51.2, 10),
            vertical_bar(51.2)
        );
    }
}
