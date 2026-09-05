//! The herdr CLI transport: spawn `herdr`, capture its JSON, never block
//! forever.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// How long any single herdr invocation may take before it is killed.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the timeout guard checks on the child.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Captured stderr is only used for messages, so keep it short enough to log.
const MAX_STDERR_CHARS: usize = 200;

/// Everything that can go wrong while talking to herdr.
#[derive(Debug)]
pub enum HerdrError {
    /// The `herdr` binary could not be started, or waiting on it failed.
    Spawn(io::Error),
    /// herdr ran but reported a failure.
    NonZeroExit { code: Option<i32>, stderr: String },
    /// herdr answered with something this client does not understand, which
    /// includes a well formed `{"error": ...}` envelope.
    Parse(String),
    /// herdr did not exit within [`COMMAND_TIMEOUT`] and was killed.
    Timeout,
}

impl fmt::Display for HerdrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "could not run the herdr CLI: {error}"),
            Self::NonZeroExit { code, stderr } => {
                let status = code.map_or_else(|| "a signal".to_string(), |code| code.to_string());
                if stderr.is_empty() {
                    write!(f, "herdr exited with {status}")
                } else {
                    write!(f, "herdr exited with {status}: {stderr}")
                }
            }
            Self::Parse(message) => write!(f, "unexpected herdr response: {message}"),
            Self::Timeout => write!(
                f,
                "herdr did not answer within {} seconds",
                COMMAND_TIMEOUT.as_secs()
            ),
        }
    }
}

impl std::error::Error for HerdrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            _ => None,
        }
    }
}

/// One open workspace, as reported by `herdr workspace list`.
///
/// herdr sends more fields than this (tab counts, agent status, ...); serde
/// ignores the ones the sampler does not need.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub focused: bool,
}

/// One `workspace report-metadata` call: the tokens to set, the ones to clear,
/// how long they live, and the sequence number that orders them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataReport {
    pub workspace_id: String,
    pub source: String,
    pub set: Vec<(String, String)>,
    pub clear: Vec<String>,
    pub ttl_ms: u64,
    pub seq: u64,
}

/// The argv for a metadata report, split out so the wire format can be tested
/// without a herdr server.
#[must_use]
pub fn report_metadata_args(report: &MetadataReport) -> Vec<String> {
    let mut args = Vec::with_capacity(9 + report.set.len() * 2 + report.clear.len() * 2);
    args.push("workspace".to_string());
    args.push("report-metadata".to_string());
    args.push(report.workspace_id.clone());
    args.push("--source".to_string());
    args.push(report.source.clone());
    for (name, value) in &report.set {
        args.push("--token".to_string());
        args.push(format!("{name}={value}"));
    }
    for name in &report.clear {
        args.push("--clear-token".to_string());
        args.push(name.clone());
    }
    args.push("--ttl-ms".to_string());
    args.push(report.ttl_ms.to_string());
    args.push("--seq".to_string());
    args.push(report.seq.to_string());
    args
}

/// The `herdr` executable this client talks to.
#[derive(Clone, Debug)]
pub struct HerdrCli {
    bin: PathBuf,
}

impl Default for HerdrCli {
    fn default() -> Self {
        Self::from_env()
    }
}

impl HerdrCli {
    /// The CLI herdr injected through `HERDR_BIN_PATH`, falling back to
    /// `herdr` resolved on `PATH` for hand-started daemons.
    #[must_use]
    pub fn from_env() -> Self {
        let bin = std::env::var_os("HERDR_BIN_PATH")
            .filter(|value| !value.is_empty())
            .map_or_else(|| PathBuf::from("herdr"), PathBuf::from);
        Self { bin }
    }

    /// A client for an explicit executable.
    #[must_use]
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into() }
    }

    /// The executable this client runs.
    #[must_use]
    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// List the open workspaces.
    ///
    /// # Errors
    ///
    /// Returns a [`HerdrError`] when herdr cannot be started, exits non-zero,
    /// takes longer than [`COMMAND_TIMEOUT`], or answers with anything other
    /// than a workspace list.
    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, HerdrError> {
        let stdout = self.run(&["workspace".to_string(), "list".to_string()])?;
        parse_workspace_list(&stdout)
    }

    /// Report one workspace's sidebar tokens.
    ///
    /// # Errors
    ///
    /// Returns a [`HerdrError`] when herdr cannot be started, exits non-zero
    /// (for example because the workspace closed mid-tick), or hangs.
    pub fn report_metadata(&self, report: &MetadataReport) -> Result<(), HerdrError> {
        self.run(&report_metadata_args(report)).map(|_| ())
    }

    /// Run `herdr` with `args`, returning its stdout.
    ///
    /// stdout and stderr are always piped rather than inherited: herdr copies a
    /// plugin's output into a capped command log, and the daemon has nothing to
    /// say there.
    fn run(&self, args: &[String]) -> Result<String, HerdrError> {
        let mut child = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(HerdrError::Spawn)?;

        wait_with_timeout(&mut child, COMMAND_TIMEOUT)?;

        // Safe to read after the child exited: both responses are a single
        // short JSON line, far below the pipe buffer that could have blocked
        // the child before it exited.
        let output = child.wait_with_output().map_err(HerdrError::Spawn)?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err(HerdrError::NonZeroExit {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(MAX_STDERR_CHARS)
                .collect(),
        })
    }
}

/// Poll `child` until it exits or the budget runs out, killing it on timeout so
/// a wedged herdr cannot freeze the sampler.
fn wait_with_timeout(child: &mut Child, budget: Duration) -> Result<(), HerdrError> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(error) => return Err(HerdrError::Spawn(error)),
        }
        if start.elapsed() >= budget {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HerdrError::Timeout);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// The response envelope every `herdr` CLI command prints.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<WorkspaceListResult>,
    #[serde(default)]
    error: Option<ErrorBody>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl ErrorBody {
    fn describe(&self) -> String {
        match (&self.code, &self.message) {
            (Some(code), Some(message)) => format!("herdr reported {code}: {message}"),
            (Some(code), None) => format!("herdr reported {code}"),
            (None, Some(message)) => format!("herdr reported an error: {message}"),
            (None, None) => "herdr reported an error".to_string(),
        }
    }
}

/// Parse the stdout of `herdr workspace list`.
///
/// # Errors
///
/// Returns [`HerdrError::Parse`] for empty output, malformed JSON, an envelope
/// carrying an `error` object, or an envelope without a workspace list.
pub fn parse_workspace_list(stdout: &str) -> Result<Vec<WorkspaceInfo>, HerdrError> {
    let payload = stdout.trim();
    if payload.is_empty() {
        return Err(HerdrError::Parse("herdr printed nothing".to_string()));
    }
    // The CLI can print progress lines before the response; the envelope is the
    // last non-empty line.
    let line = payload
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(payload)
        .trim();

    let envelope: Envelope =
        serde_json::from_str(line).map_err(|error| HerdrError::Parse(error.to_string()))?;
    if let Some(error) = envelope.error {
        return Err(HerdrError::Parse(error.describe()));
    }
    envelope
        .result
        .map(|result| result.workspaces)
        .ok_or_else(|| HerdrError::Parse("response carried no result".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{parse_workspace_list, report_metadata_args, HerdrCli, HerdrError, MetadataReport};

    /// A real `herdr workspace list` payload, widened to two workspaces. The
    /// extra fields are exactly the ones herdr 0.8.2 sends and this client
    /// ignores.
    const WORKSPACE_LIST: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"w1:t1","agent_status":"idle","focused":false,"label":"~","number":1,"pane_count":1,"tab_count":1,"workspace_id":"w1"},{"active_tab_id":"w9:t3","agent_status":"working","focused":true,"label":"herdr","number":2,"pane_count":4,"tab_count":3,"workspace_id":"w9"}]}}"#;

    const ERROR_PAYLOAD: &str = r#"{"id":"cli:workspace:list","error":{"code":"not_connected","message":"no herdr server"}}"#;

    #[test]
    fn parses_a_workspace_list_ignoring_unknown_fields() {
        let workspaces = parse_workspace_list(WORKSPACE_LIST).expect("payload parses");
        assert_eq!(workspaces.len(), 2);

        assert_eq!(workspaces[0].workspace_id, "w1");
        assert_eq!(workspaces[0].label, "~");
        assert_eq!(workspaces[0].number, 1);
        assert!(!workspaces[0].focused);

        assert_eq!(workspaces[1].workspace_id, "w9");
        assert_eq!(workspaces[1].label, "herdr");
        assert_eq!(workspaces[1].number, 2);
        assert!(workspaces[1].focused);
    }

    #[test]
    fn an_error_envelope_is_a_parse_error() {
        let error = parse_workspace_list(ERROR_PAYLOAD).expect_err("error payload fails");
        match error {
            HerdrError::Parse(message) => {
                assert!(message.contains("not_connected"), "{message}");
                assert!(message.contains("no herdr server"), "{message}");
            }
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn malformed_and_empty_output_are_parse_errors() {
        for payload in ["", "   \n", "not json", "{\"id\":\"cli:workspace:list\"}"] {
            assert!(
                matches!(parse_workspace_list(payload), Err(HerdrError::Parse(_))),
                "expected {payload:?} to be a parse error"
            );
        }
    }

    #[test]
    fn report_metadata_args_have_the_documented_order() {
        let report = MetadataReport {
            workspace_id: "w1".to_string(),
            source: "system-monitor".to_string(),
            set: vec![
                ("cpu_status".to_string(), "[|||||     ] 51.2%".to_string()),
                (
                    "mem_status".to_string(),
                    "[|||       ] 2885/7987MB".to_string(),
                ),
            ],
            clear: vec!["cpu_warn".to_string(), "cpu_hot".to_string()],
            ttl_ms: 5000,
            seq: 42,
        };

        assert_eq!(
            report_metadata_args(&report),
            vec![
                "workspace",
                "report-metadata",
                "w1",
                "--source",
                "system-monitor",
                "--token",
                "cpu_status=[|||||     ] 51.2%",
                "--token",
                "mem_status=[|||       ] 2885/7987MB",
                "--clear-token",
                "cpu_warn",
                "--clear-token",
                "cpu_hot",
                "--ttl-ms",
                "5000",
                "--seq",
                "42",
            ]
        );
    }

    #[test]
    fn the_client_prefers_the_injected_binary_path() {
        assert_eq!(
            HerdrCli::new("/opt/herdr/bin/herdr").bin(),
            std::path::Path::new("/opt/herdr/bin/herdr")
        );
    }

    #[test]
    fn a_missing_binary_is_a_spawn_error() {
        let cli = HerdrCli::new("herdr-that-does-not-exist");
        assert!(matches!(cli.list_workspaces(), Err(HerdrError::Spawn(_))));
    }
}
