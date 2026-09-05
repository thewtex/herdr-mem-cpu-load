# Phase 05: CI, Release Automation, and Hardening

This phase makes the project shippable and trustworthy. GitHub Actions runs format, lint, tests, and a smoke test of the real binary on Linux, macOS, and Windows, which is the first time the macOS and Windows backends from Phase 03 actually execute. A tag-driven release workflow publishes prebuilt binaries, integration tests exercise the CLI end to end, and a final review pass tightens error handling, documentation, and repository hygiene so `herdr plugin install thewtex/herdr-mem-cpu-load` works for anyone with a Rust toolchain.

## Tasks

- [ ] Add end-to-end integration tests in `tests/cli.rs` that run the built binary through `env!("CARGO_BIN_EXE_herdr-mem-cpu-load")` with `std::process::Command` (no extra test dependencies):
  - Default invocation with `--interval 1` exits 0 and stdout matches the one-line regex from Phase 01; `--graph-style blocks` output contains `▕` and `▏`; `-v` output contains exactly one character from ` ▁▂▃▄▅▆▇█▲` between the frame characters; `-m 2` prints a percent; `-a 0` prints no load averages; `-t 1` on a multi-core machine can exceed 100 percent (assert only that it parses as a number).
  - Invalid arguments (`-a 5`, `-m 9`, `--graph-style nope`, `-i 0`) exit non-zero with a message on stderr and nothing on stdout.
  - `--print-config` emits TOML that round-trips through `toml::from_str::<FileConfig>` (add `toml` as a dev-dependency only if it is not already a normal dependency).
  - `--daemon` with `HERDR_BIN_PATH` pointing at a small fake script written by the test into a temp dir (a shell script on Unix, a `.cmd` on Windows) that records its argv to a file and answers `workspace list` with a canned two-workspace JSON: run the daemon with `--interval 1` for 3 seconds, kill it, and assert the recorded argv contains `report-metadata` calls for both workspace ids with `--source system-monitor`, `--ttl-ms`, `--seq`, and the expected `--token` names. Use `cfg(unix)`/`cfg(windows)` helpers to write the fake in the right language.
  - Verify the fake-herdr "server gone" path: make the fake exit 1 and assert the daemon exits 0 within `max_failures * interval + 2` seconds.

- [ ] Create `.github/workflows/ci.yml` modeled on the structure of `/home/matt/src/herdr/.github/workflows/ci.yml` but much smaller:
  - Trigger on `push` to `main` and on `pull_request`; `permissions: contents: read`; concurrency group cancelling in-progress runs per ref.
  - Job `check` with a matrix of `ubuntu-latest`, `macos-latest`, and `windows-latest`: checkout (pin actions by SHA like the herdr workflow does), install the stable toolchain via `dtolnay/rust-toolchain@stable` with `clippy` and `rustfmt` components, cache with `Swatinem/rust-cache`, then `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `cargo build --release --locked`, and a smoke step that runs the release binary with `--interval 1` and `--graph-style blocks` and prints the output (use `bash` shell on all three runners so the step is identical).
  - Job `cross-check` on `ubuntu-latest` running `cargo check` for `x86_64-unknown-freebsd` to keep the unsupported stub compiling (allow it to be skipped if the target's rust-std is unavailable, using `continue-on-error: true`).
  - Job `manifest` on `ubuntu-latest` that validates `herdr-plugin.toml` parses as TOML and contains the required keys (`id`, `name`, `version`, `min_herdr_version`) using a short Python script, and that `version` in `herdr-plugin.toml` equals `version` in `Cargo.toml`.

- [ ] Create `.github/workflows/release.yml` triggered by tags matching `v*`:
  - Build release binaries for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (via `cross` or the `aarch64` GitHub runner if available; otherwise skip that target), `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, and `aarch64-pc-windows-msvc`, package each as `herdr-mem-cpu-load-<version>-<target>.tar.gz` (or `.zip` on Windows) containing the binary, `README.md`, and `LICENSE`, and attach them to a GitHub release created with `softprops/action-gh-release` (pinned by SHA) using the tag's CHANGELOG section as the body.
  - Add a `scripts/release.sh` (Unix) that bumps the version in `Cargo.toml`, `Cargo.lock`, and `herdr-plugin.toml` together, updates `CHANGELOG.md`, and creates the annotated tag, so the three version numbers cannot drift. Document it in `CONTRIBUTING.md`.

- [ ] Add repository hygiene files and metadata:
  - `CHANGELOG.md` in Keep a Changelog format with an `Unreleased` section summarizing everything built in Phases 01–04 (one-line port, daemon tokens, macOS/Windows backends, config, colors, watch popup).
  - `CONTRIBUTING.md`: build/test commands, the cross-target `cargo check` commands from Phase 03, how to test against a live herdr (`plugin link`, running the daemon by hand, `herdr plugin log list`), conventional commit subjects, and the release script.
  - `.editorconfig` (UTF-8, LF, 4-space Rust indentation, 2-space TOML/YAML/Markdown).
  - Update `Cargo.toml` `include` list so `cargo package` ships only `src/**`, `README.md`, `LICENSE`, `CHANGELOG.md`, and `herdr-plugin.toml`; run `cargo package --allow-dirty --list` to confirm.
  - README: add CI and license badges, and a "Publishing to the herdr marketplace" note explaining that the GitHub repository needs the `herdr-plugin` topic and the manifest at the repository root for automatic indexing (see `/home/matt/src/herdr/docs/next/website/src/content/docs/marketplace.mdx`).

- [ ] Hardening pass over the whole crate, reading every module in `src/` with fresh eyes:
  - Replace any remaining `unwrap`/`expect` outside tests with proper error propagation; the daemon must never panic.
  - Make sure every `unsafe` block has a `// SAFETY:` comment and is minimal; run `cargo clippy --all-targets -- -D warnings -W clippy::undocumented_unsafe_blocks`.
  - Confirm `build_tokens` output never exceeds 80 characters even with `graph_lines = 60` (clamp the bar width so the widest token fits; add a unit test with extreme widths) and that a `graph_lines` above 64 is rejected by CLI validation with a helpful message.
  - Check that `--interval` larger than 3600 is rejected and that `ttl_ms` is always clamped to herdr's accepted range 1–86400000.
  - Run the daemon under `valgrind --tool=none`-free alternatives: watch RSS with `ps -o rss` over 60 seconds at `--interval 1` and confirm it stays flat (no growth from the history ring or log handles).
  - Run `cargo doc --no-deps` and fix broken intra-doc links; add crate-level docs in `src/main.rs` describing the module map.

- [ ] Final verification and commit:
  - `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` (unit + integration) all pass on this machine, and the four cross-target `cargo check` commands from Phase 03 still pass.
  - `cargo build --release --locked`, then `herdr plugin link /home/matt/src/herdr-mem-cpu-load`, start the daemon by hand for 5 seconds, and confirm tokens appear in `herdr workspace list` one more time.
  - Commit with the message `ci: add cross-platform CI, release workflow, integration tests, and hardening`. Do not push or tag; leave that to the user after they add a GitHub remote.
