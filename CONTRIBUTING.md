# Contributing

Thanks for helping. This is a small crate with no build script, no code
generation, and no dependency that needs a C toolchain — `cargo` is the whole
build system.

## Requirements

- Rust 1.85 or newer (`rust-version` in `Cargo.toml`).
- Python 3.11 or newer for `scripts/check_manifest.py`, which CI runs. Only
  the standard library is used.
- herdr 0.8.0 or newer to exercise the plugin side.

## Build and test

```sh
cargo build
cargo test
cargo run -- --interval 1
```

The same four checks CI runs, in the order it runs them:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

`cargo test` covers both the unit tests inside `src/` and the end-to-end tests
in `tests/cli.rs`, which spawn the built binary — including two that run the
daemon for a few seconds against a fake `herdr` the test writes into a
temporary directory. They take about five seconds; nothing there needs a real
herdr server.

## Cross-target checks

This machine cannot run every backend, so every declared platform is
type-checked instead. `cargo check` does not link, so no cross linker is
needed:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin \
    x86_64-pc-windows-msvc x86_64-pc-windows-gnu x86_64-unknown-freebsd

cargo check --target aarch64-apple-darwin
cargo check --target x86_64-apple-darwin
cargo check --target x86_64-pc-windows-msvc
cargo check --target x86_64-pc-windows-gnu
cargo check --target x86_64-unknown-freebsd   # the unsupported-platform stub
```

Run clippy against each too when touching `src/sys/`:

```sh
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

CI actually *runs* the macOS and Windows backends on their own runners, which
is the only place they execute; the FreeBSD stub is check-only there as well.

## Testing against a live herdr

`plugin link` does not run the manifest's build commands, so build the release
binary the startup hook points at first:

```sh
cargo build --release
herdr plugin link /path/to/herdr-mem-cpu-load
herdr plugin list
```

Startup hooks only run when a herdr *server* starts — not when a client
attaches, the config reloads, or a plugin is linked. After linking, either
restart herdr or start the daemon by hand:

```sh
herdr-mem-cpu-load --daemon --log-file /tmp/mem-cpu-load.log --verbose &
herdr workspace list          # the tokens should appear on every workspace
tail -f /tmp/mem-cpu-load.log
```

What herdr saw from the plugin:

```sh
herdr plugin log list --plugin thewtex.mem-cpu-load
```

The daemon writes nothing to stdout on purpose — herdr copies a plugin's
output into a capped command log — so diagnostics go to `--log-file`, or to
`daemon.log` in `$HERDR_PLUGIN_STATE_DIR` when herdr started it. Only one
daemon runs at a time; a second one logs
`another herdr-mem-cpu-load daemon is already running` and exits 0.

The popup pane and the two actions:

```sh
herdr plugin pane open --plugin thewtex.mem-cpu-load --entrypoint status
herdr plugin action list --plugin thewtex.mem-cpu-load
```

## Commit messages

Conventional commit subjects, matching the herdr repository:

```
feat: add a vertical graph style
fix: stop the daemon panicking on a closed log file
docs: explain the Windows load average
ci: pin every action by SHA
chore: release v0.2.0
```

The type is one of `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`,
`build`, or `chore`, optionally with a scope (`fix(daemon): ...`). Keep the
subject in the imperative and under about 72 characters, and put the reasoning
in the body.

## Releasing

The version lives in three files — `Cargo.toml`, `Cargo.lock`, and
`herdr-plugin.toml` — and herdr's marketplace reads the last one while `cargo`
reads the first. `scripts/release.sh` bumps all three together, closes out the
`## [Unreleased]` section of `CHANGELOG.md`, commits, and creates the annotated
tag:

```sh
scripts/release.sh --dry-run 0.2.0   # show what would change
scripts/release.sh 0.2.0
```

Nothing is pushed. Review the commit, then:

```sh
git push origin HEAD
git push origin v0.2.0
```

Pushing the tag is what runs `.github/workflows/release.yml`, which builds a
binary for every supported target, packages each with `README.md` and
`LICENSE`, and attaches the archives to a GitHub release whose body is that
version's changelog section.

Do not bump a version by hand. `scripts/check_manifest.py` runs in CI and fails
the build when `herdr-plugin.toml` and `Cargo.toml` disagree.
