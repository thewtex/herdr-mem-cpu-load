//! CPU, memory, and load average sampling and rendering for herdr.
//!
//! Re-imagined in Rust from `tmux-mem-cpu-load`. The layers are:
//!
//! * [`sys`] — the platform abstraction over raw OS counters.
//! * [`metrics`] — platform independent sampling built on top of it.
//! * [`render`] — bar graphs and text formatting for the status line.
//! * [`tokens`] — the Space sidebar tokens a sample turns into.
//! * [`herdr`] — the client that talks to a running herdr server.
//! * [`daemon`] — the sampling loop that ties those last two together.
//! * [`cli`] — the `tmux-mem-cpu-load` compatible command line.
//! * [`config`] — the `config.toml` layer the command line is merged over.
//! * [`watch`] — the live status line the herdr popup pane runs.
//!
//! The `herdr-mem-cpu-load` binary is a thin wrapper over these: without
//! `--daemon` it prints one status line and exits, with `--daemon` it runs the
//! loop in [`daemon::run`].

pub mod cli;
pub mod config;
pub mod daemon;
pub mod herdr;
pub mod metrics;
pub mod render;
pub mod sys;
pub mod tokens;
pub mod watch;
