//! A small client for the herdr server.
//!
//! Everything goes through the `herdr` CLI rather than the raw socket: the
//! transport behind `HERDR_SOCKET_PATH` is a Unix socket on Unix and a named
//! pipe on Windows, and the plugin documentation recommends `HERDR_BIN_PATH`
//! precisely so plugins do not have to care about that difference.

pub mod cli;

pub use cli::{
    report_metadata_args, HerdrCli, HerdrError, MetadataReport, WorkspaceInfo, COMMAND_TIMEOUT,
};
