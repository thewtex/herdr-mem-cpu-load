//! CPU, memory, and load average sampling and rendering for herdr.
//!
//! Re-imagined in Rust from `tmux-mem-cpu-load`. The layers are:
//!
//! * [`sys`] — the platform abstraction over raw OS counters.
//! * [`metrics`] — platform independent sampling built on top of it.
//! * [`render`] — bar graphs and text formatting for the status line.
//! * [`cli`] — the `tmux-mem-cpu-load` compatible command line.
//!
//! The `herdr-mem-cpu-load` binary is a thin wrapper over these; the library
//! surface is what the herdr daemon mode builds on.

pub mod cli;
pub mod metrics;
pub mod render;
pub mod sys;
