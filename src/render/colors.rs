//! tmux and ANSI colours for the one-line status, ported from the original's
//! `common/luts.h` and `common/powerline.cc`.
//!
//! `luts.h` is 303 string literals holding three 101 entry tables. Rather than
//! paste them, this module keeps the *generator* — `generate-luts.py` picks a
//! colour off a matplotlib colormap, quantises it into the xterm 6x6x6 cube
//! with `16 + 36*red + 6*green + blue`, and takes `brightwhite` as the
//! foreground below 50% and `black` at 50% and above — and stores each table
//! run-length encoded in cube coordinates. Forty-eight runs replace the three
//! hundred literals and reproduce the header byte for byte.
//!
//! The tables are not regenerated from a colormap at build time for two
//! reasons. `cpu_percentage_lut` stopped being one when upstream commit
//! 04419c8 ("Progressive colors for CPU usage") replaced it with a hand
//! authored black -> blue -> violet -> red ramp that also dropped the
//! foreground flip, so it keeps `brightwhite` all the way to 100%. And pulling
//! matplotlib in to re-derive `gist_earth` and `bone` would be a build
//! dependency for 101 constants that have not changed since 2016.

use std::fmt;

/// The foreground half of a lookup table entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Foreground {
    /// Used below 50%, where the background is dark.
    #[default]
    BrightWhite,
    /// Used at 50% and above, where the background is light.
    Black,
}

impl Foreground {
    /// The tmux colour name.
    #[must_use]
    pub const fn tmux(self) -> &'static str {
        match self {
            Self::BrightWhite => "brightwhite",
            Self::Black => "black",
        }
    }

    /// The xterm palette index the name refers to.
    #[must_use]
    pub const fn ansi(self) -> u16 {
        match self {
            Self::BrightWhite => 15,
            Self::Black => 0,
        }
    }
}

/// One lookup table entry: a foreground name and a 256-colour background.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub fg: Foreground,
    pub bg: u16,
}

impl Color {
    /// The tmux markup, e.g. `#[fg=brightwhite,bg=colour56]`.
    #[must_use]
    pub fn tmux(self) -> String {
        format!("#[fg={},bg=colour{}]", self.fg.tmux(), self.bg)
    }

    /// The equivalent 256-colour ANSI escapes.
    #[must_use]
    pub fn ansi(self) -> String {
        format!("\x1b[38;5;{}m\x1b[48;5;{}m", self.fg.ansi(), self.bg)
    }
}

/// What the original prints after a coloured segment.
pub const TMUX_RESET: &str = "#[fg=default,bg=default]";
/// The ANSI equivalent of [`TMUX_RESET`].
pub const ANSI_RESET: &str = "\x1b[0m";
/// The powerline separator `-p` draws, U+E0B0.
pub const PWL_LEFT_FILLED: &str = "\u{e0b0}";
/// The powerline separator `-q` draws, U+E0B2.
pub const PWL_RIGHT_FILLED: &str = "\u{e0b2}";

/// Which colour markup the one-line renderer emits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Plain text, byte for byte what the uncoloured original prints.
    #[default]
    None,
    /// `#[fg=...,bg=...]` markup for a tmux status line.
    Tmux,
    /// SGR escapes for a plain terminal.
    Ansi,
}

/// Which powerline separators the tmux markup blends with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PowerlineMode {
    /// No separators.
    #[default]
    None,
    /// `-p`: left pointing separators.
    Left,
    /// `-q`: right pointing separators.
    Right,
}

/// `16 + 36*red + 6*green + blue` from `generate-luts.py`, over the xterm
/// 6x6x6 colour cube.
#[must_use]
pub const fn cube_index(red: u8, green: u8, blue: u8) -> u16 {
    16 + 36 * red as u16 + 6 * green as u16 + blue as u16
}

/// One run of equal entries in a lookup table: how many percentages it covers
/// and the cube coordinate they share.
type Run = (u8, (u8, u8, u8));

/// One of the three tables in `luts.h`.
struct Lut {
    runs: &'static [Run],
    /// Whether the foreground turns black at 50%, as `generate-luts.py`
    /// writes it. The hand authored CPU table does not.
    flips_at_half: bool,
}

/// `cpu_percentage_lut`: black, up through the blues to violet, then down to
/// red. Hand authored upstream rather than sampled from a colormap.
const CPU_RUNS: [Run; 16] = [
    (7, (0, 0, 0)),
    (8, (0, 0, 1)),
    (7, (0, 0, 2)),
    (6, (0, 0, 3)),
    (7, (0, 0, 4)),
    (6, (0, 0, 5)),
    (6, (1, 0, 5)),
    (6, (1, 0, 4)),
    (6, (1, 0, 3)),
    (6, (1, 0, 2)),
    (6, (1, 0, 1)),
    (6, (1, 0, 0)),
    (6, (2, 0, 0)),
    (6, (3, 0, 0)),
    (6, (4, 0, 0)),
    (6, (5, 0, 0)),
];

/// `mem_lut`: matplotlib's `gist_earth`, ocean through land to snow.
const MEM_RUNS: [Run; 19] = [
    (1, (0, 0, 0)),
    (1, (0, 0, 1)),
    (6, (0, 0, 2)),
    (8, (0, 1, 2)),
    (1, (1, 1, 2)),
    (12, (1, 2, 2)),
    (16, (1, 3, 2)),
    (3, (1, 3, 1)),
    (4, (2, 3, 1)),
    (5, (2, 3, 2)),
    (10, (3, 3, 2)),
    (2, (3, 4, 2)),
    (3, (4, 4, 2)),
    (11, (4, 3, 2)),
    (4, (4, 3, 3)),
    (4, (4, 4, 3)),
    (1, (4, 4, 4)),
    (5, (5, 4, 4)),
    (4, (5, 5, 5)),
];

/// `load_lut`: matplotlib's `bone`, black through slate to white.
const LOAD_RUNS: [Run; 13] = [
    (9, (0, 0, 0)),
    (3, (0, 0, 1)),
    (13, (1, 1, 1)),
    (10, (1, 1, 2)),
    (8, (2, 2, 2)),
    (9, (2, 2, 3)),
    (6, (2, 3, 3)),
    (8, (3, 3, 3)),
    (2, (3, 3, 4)),
    (11, (3, 4, 4)),
    (10, (4, 4, 4)),
    (4, (4, 5, 5)),
    (8, (5, 5, 5)),
];

/// How many percentages a lookup table covers: 0 through 100 inclusive.
pub const LUT_LEN: u32 = 101;

const CPU_LUT: Lut = Lut {
    runs: &CPU_RUNS,
    flips_at_half: false,
};
const MEM_LUT: Lut = Lut {
    runs: &MEM_RUNS,
    flips_at_half: true,
};
const LOAD_LUT: Lut = Lut {
    runs: &LOAD_RUNS,
    flips_at_half: true,
};

/// Walk `lut`'s runs far enough to answer for `percent`, which is clamped into
/// `0..=100` the way the original clamps its load percentage.
fn lookup(lut: &Lut, percent: u32) -> Color {
    let percent = percent.min(LUT_LEN - 1);
    let fg = if lut.flips_at_half && percent >= 50 {
        Foreground::Black
    } else {
        Foreground::BrightWhite
    };

    let mut remaining = percent;
    for (len, (red, green, blue)) in lut.runs {
        if remaining < u32::from(*len) {
            return Color {
                fg,
                bg: cube_index(*red, *green, *blue),
            };
        }
        remaining -= u32::from(*len);
    }
    // Unreachable: every table covers all 101 percentages, which the unit
    // tests assert. Falling back to the last run keeps this function total.
    let (_, (red, green, blue)) = lut.runs[lut.runs.len() - 1];
    Color {
        fg,
        bg: cube_index(red, green, blue),
    }
}

/// The colour the original paints the CPU segment for a truncated percentage.
#[must_use]
pub fn cpu_color(percent: u32) -> Color {
    lookup(&CPU_LUT, percent)
}

/// The colour the original paints the memory segment for a truncated
/// used-memory percentage.
#[must_use]
pub fn mem_color(percent: u32) -> Color {
    lookup(&MEM_LUT, percent)
}

/// The colour the original paints the load segment for
/// [`crate::metrics::load::load_percent`].
#[must_use]
pub fn load_color(percent: u32) -> Color {
    lookup(&LOAD_LUT, percent)
}

/// `bg2fg` from `powerline.cc`: turn `#[fg=x,bg=y]` into `#[fg=y]` so the
/// separator glyph is drawn in the colour of the segment behind it.
#[must_use]
pub fn bg2fg(color: &str) -> String {
    match color.find(',') {
        // The C++ builds "#[f" + (strchr(s, ',') + 2), dropping the comma and
        // the `b` of `bg=`.
        Some(comma) => format!("#[f{}", &color[comma + 2..]),
        None => color.to_string(),
    }
}

/// The `bg=...` half of `#[fg=x,bg=y]`, wrapped back up on its own.
fn bg_only(color: &str) -> String {
    match color.find(',') {
        Some(comma) => format!("#[{}", &color[comma + 1..]),
        None => color.to_string(),
    }
}

/// `powerline()` from `powerline.cc`.
///
/// `background_only` is the left-pointing end of a segment, where only the
/// background needs inverting so the next segment's separator can be drawn.
#[must_use]
pub fn powerline(color: &str, direction: PowerlineMode, background_only: bool) -> String {
    match direction {
        PowerlineMode::None => color.to_string(),
        PowerlineMode::Left => {
            if background_only {
                format!(" {}", bg2fg(color))
            } else {
                format!("{}{PWL_LEFT_FILLED}{color}", bg_only(color))
            }
        }
        PowerlineMode::Right => format!(" {}{PWL_RIGHT_FILLED}{color}", bg2fg(color)),
    }
}

/// `powerline_char()` from `powerline.cc`: blend a segment with the fixed
/// colour of whatever tmux draws next to it.
#[must_use]
pub fn powerline_char(
    color: &str,
    static_color: u16,
    direction: PowerlineMode,
    eol: bool,
) -> String {
    match direction {
        PowerlineMode::None => String::new(),
        PowerlineMode::Left => {
            let head = if eol {
                format!("{}#[bg=colour{static_color}]", bg2fg(color))
            } else {
                format!("{color}#[fg=colour{static_color}]")
            };
            format!("{head}{PWL_LEFT_FILLED}{color}")
        }
        PowerlineMode::Right => {
            let head = if eol {
                format!("{color}#[fg=colour{static_color}] ")
            } else {
                format!("{} #[bg=colour{static_color}]", bg2fg(color))
            };
            format!("{head}{PWL_RIGHT_FILLED}{color}")
        }
    }
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::Tmux => "tmux",
            Self::Ansi => "ansi",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bg2fg, cpu_color, cube_index, load_color, lookup, mem_color, powerline, powerline_char,
        Color, Foreground, PowerlineMode, CPU_RUNS, LOAD_LUT, LOAD_RUNS, LUT_LEN, MEM_RUNS,
    };

    fn total(runs: &[(u8, (u8, u8, u8))]) -> u32 {
        runs.iter().map(|(len, _)| u32::from(*len)).sum()
    }

    #[test]
    fn every_table_covers_all_one_hundred_and_one_percentages() {
        for runs in [&CPU_RUNS[..], &MEM_RUNS[..], &LOAD_RUNS[..]] {
            assert_eq!(total(runs), LUT_LEN);
        }
    }

    #[test]
    fn the_cube_index_matches_the_generator() {
        // 16 + 36*red + 6*green + blue over the xterm 6x6x6 cube.
        assert_eq!(cube_index(0, 0, 0), 16);
        assert_eq!(cube_index(1, 0, 4), 56);
        assert_eq!(cube_index(5, 0, 0), 196);
        assert_eq!(cube_index(5, 5, 5), 231);
    }

    /// Spot checks against `common/luts.h` in the C++ original.
    #[test]
    fn lookup_tables_match_the_original_header() {
        assert_eq!(cpu_color(0).bg, 16);
        assert_eq!(cpu_color(50).bg, 56);
        assert_eq!(cpu_color(100).bg, 196);
        // The runs either side of the 50% entry.
        assert_eq!(cpu_color(46).bg, 57);
        assert_eq!(cpu_color(53).bg, 55);

        assert_eq!(mem_color(0).bg, 16);
        assert_eq!(mem_color(50).bg, 107);
        // The last entry of mem_lut is pure white.
        assert_eq!(mem_color(100).bg, 231);

        assert_eq!(load_color(0).bg, 16);
        assert_eq!(load_color(50).bg, 103);
        // As is the last entry of load_lut.
        assert_eq!(load_color(100).bg, 231);
    }

    #[test]
    fn the_foreground_flips_at_the_halfway_point() {
        assert_eq!(mem_color(49).fg, Foreground::BrightWhite);
        assert_eq!(mem_color(50).fg, Foreground::Black);
        assert_eq!(load_color(49).fg, Foreground::BrightWhite);
        assert_eq!(load_color(50).fg, Foreground::Black);
        // The hand authored CPU table stayed brightwhite throughout.
        assert_eq!(cpu_color(49).fg, Foreground::BrightWhite);
        assert_eq!(cpu_color(50).fg, Foreground::BrightWhite);
        assert_eq!(cpu_color(100).fg, Foreground::BrightWhite);
    }

    #[test]
    fn out_of_range_percentages_are_clamped() {
        assert_eq!(cpu_color(1000), cpu_color(100));
        assert_eq!(lookup(&LOAD_LUT, LUT_LEN), load_color(100));
    }

    #[test]
    fn markup_is_rendered_for_tmux_and_for_ansi() {
        let color = Color {
            fg: Foreground::BrightWhite,
            bg: 56,
        };
        assert_eq!(color.tmux(), "#[fg=brightwhite,bg=colour56]");
        assert_eq!(color.ansi(), "\x1b[38;5;15m\x1b[48;5;56m");

        let dark = Color {
            fg: Foreground::Black,
            bg: 231,
        };
        assert_eq!(dark.tmux(), "#[fg=black,bg=colour231]");
        assert_eq!(dark.ansi(), "\x1b[38;5;0m\x1b[48;5;231m");
    }

    #[test]
    fn bg2fg_moves_the_background_into_the_foreground() {
        assert_eq!(bg2fg("#[fg=brightwhite,bg=colour56]"), "#[fg=colour56]");
        assert_eq!(bg2fg("#[fg=default,bg=default]"), "#[fg=default]");
    }

    #[test]
    fn powerline_matches_the_c_plus_plus_control_flow() {
        let color = "#[fg=brightwhite,bg=colour56]";

        assert_eq!(powerline(color, PowerlineMode::None, false), color);
        assert_eq!(
            powerline(color, PowerlineMode::Left, false),
            format!("#[bg=colour56]\u{e0b0}{color}")
        );
        assert_eq!(
            powerline(color, PowerlineMode::Left, true),
            " #[fg=colour56]"
        );
        assert_eq!(
            powerline(color, PowerlineMode::Right, false),
            format!(" #[fg=colour56]\u{e0b2}{color}")
        );
    }

    #[test]
    fn powerline_char_blends_with_a_fixed_neighbour() {
        let color = "#[fg=brightwhite,bg=colour56]";

        assert_eq!(
            powerline_char(color, 4, PowerlineMode::Left, false),
            format!("{color}#[fg=colour4]\u{e0b0}{color}")
        );
        assert_eq!(
            powerline_char(color, 4, PowerlineMode::Left, true),
            format!("#[fg=colour56]#[bg=colour4]\u{e0b0}{color}")
        );
        assert_eq!(
            powerline_char(color, 8, PowerlineMode::Right, false),
            format!("#[fg=colour56] #[bg=colour8]\u{e0b2}{color}")
        );
        assert_eq!(
            powerline_char(color, 8, PowerlineMode::Right, true),
            format!("{color}#[fg=colour8] \u{e0b2}{color}")
        );
    }
}
