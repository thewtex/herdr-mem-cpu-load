//! Seeding herdr's own `config.toml` with the plugin's Space sidebar rows.
//!
//! The daemon reports tokens, but a token only reaches the sidebar once
//! `[ui.sidebar.spaces]` names it, and that table lives in herdr's
//! configuration file rather than the plugin's. So the plugin's install runs
//! `--write-sidebar-rows`, which puts a working layout there and a fresh
//! install grows the rows on its own instead of asking the reader to copy a
//! block out of the README.
//!
//! Nothing here overwrites a layout. Rows that are already set, a file that
//! does not parse, and a `[ui]` that is not a table are all left exactly as
//! they are: this plugin does not own that file, and a sidebar someone
//! arranged by hand is not ours to rearrange.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Key, Table};

use crate::config::{ConfigError, CONFIG_FILE_NAME};

/// The full path to herdr's configuration file, when herdr was pointed at one.
pub const CONFIG_PATH_ENV: &str = "HERDR_CONFIG_PATH";
/// The configuration home herdr's own directory sits inside, when it is set.
pub const XDG_CONFIG_HOME_ENV: &str = "XDG_CONFIG_HOME";
/// herdr's directory name inside whichever configuration home applies.
pub const HERDR_DIR_NAME: &str = "herdr";

/// The table the rows belong to, a key at a time.
const TABLE_PATH: [&str; 3] = ["ui", "sidebar", "spaces"];
/// The key inside it that holds the layout.
const ROWS_KEY: &str = "rows";
/// How far a chain of symbolic links is followed before giving up on it.
const MAX_SYMLINK_HOPS: usize = 16;

/// The rows written into a `[ui.sidebar.spaces]` that has none, comment and
/// formatting and all: the file is copied into someone's configuration
/// verbatim, so it is written out here rather than built up in code.
pub const SPACES_ROWS: &str = include_str!("sidebar/spaces_rows.toml");

/// What a run of [`write_rows`] came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The rows were added to the file, which was created if it was missing.
    Written,
    /// Something already sets `ui.sidebar.spaces.rows`, so the file was left
    /// untouched.
    AlreadySet,
}

/// The `config.toml` herdr itself reads, resolved the way herdr resolves it.
#[must_use]
pub fn config_path() -> PathBuf {
    resolve_config_path(
        env_path(CONFIG_PATH_ENV),
        env_path(XDG_CONFIG_HOME_ENV),
        platform_config_dir(),
    )
}

/// An environment variable as a path, treating unset and empty alike.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// herdr's search order: an explicit path, then a configuration home it hangs
/// its own directory under, then the platform's.
fn resolve_config_path(
    explicit: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    platform_dir: Option<PathBuf>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(home) = xdg_config_home {
        return home.join(HERDR_DIR_NAME).join(CONFIG_FILE_NAME);
    }
    platform_dir
        .unwrap_or_else(|| std::env::temp_dir().join(HERDR_DIR_NAME))
        .join(CONFIG_FILE_NAME)
}

#[cfg(windows)]
fn platform_config_dir() -> Option<PathBuf> {
    if let Some(appdata) = env_path("APPDATA") {
        return Some(appdata.join(HERDR_DIR_NAME));
    }
    if let Some(profile) = env_path("USERPROFILE") {
        return Some(profile.join("AppData").join("Roaming").join(HERDR_DIR_NAME));
    }
    env_path("HOME").map(|home| home.join(".config").join(HERDR_DIR_NAME))
}

#[cfg(not(windows))]
fn platform_config_dir() -> Option<PathBuf> {
    env_path("HOME").map(|home| home.join(".config").join(HERDR_DIR_NAME))
}

/// Put the rows in `path` unless a layout is already there.
///
/// A missing file is written from scratch; an existing one keeps every key,
/// comment, and blank line it had, because only the one table is touched.
///
/// # Errors
///
/// Returns a [`ConfigError`] when the file cannot be read or written, when it
/// does not parse as TOML, or when `ui`, `ui.sidebar`, or `ui.sidebar.spaces`
/// is something other than a table. Every one of those leaves the file as it
/// was.
pub fn write_rows(path: &Path) -> Result<Outcome, ConfigError> {
    let fail = |message: String| ConfigError {
        path: path.to_path_buf(),
        message,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(fail(error.to_string())),
    };

    let mut document: DocumentMut = text
        .parse()
        .map_err(|error: toml_edit::TomlError| fail(format!("{error}; leaving it alone")))?;
    if !insert_rows(&mut document).map_err(fail)? {
        return Ok(Outcome::AlreadySet);
    }

    // Someone could have saved the file themselves since it was read a few
    // microseconds ago. Reading it again costs nothing next to losing an edit,
    // so a file that moved underneath is left the way they left it.
    if std::fs::read_to_string(path).unwrap_or_default() != text {
        return Err(fail(
            "changed while the rows were being added; leaving it alone".to_string(),
        ));
    }
    replace_contents(path, &document.to_string()).map_err(|error| fail(error.to_string()))?;
    Ok(Outcome::Written)
}

/// Put `contents` at `path` in one step, or leave what is there alone.
///
/// Someone's herdr configuration is not a file to half write: a truncating
/// write that stops for a full disk or a killed install would take the whole
/// thing with it. So the new text is written beside the old, flushed, and
/// renamed over it, which is the point at which the change becomes visible.
///
/// The rename lands on what `path` resolves to rather than on `path` itself,
/// so a `config.toml` symlinked out of a dotfiles repository has the file it
/// names rewritten instead of the link replaced by a regular file. The old
/// file's permissions are carried over for the same reason: nothing about the
/// file should come back different except the rows.
fn replace_contents(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;

    let target = resolve_target(path);
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let name = target.file_name().map_or_else(
        || CONFIG_FILE_NAME.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let temporary =
        target.with_file_name(format!(".{name}.herdr-mem-cpu-load-{}", std::process::id()));

    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(contents.as_bytes())?;
        // Renaming a file whose contents are still in flight would trade a
        // half written configuration for an empty one.
        file.sync_all()?;
        drop(file);
        if let Ok(existing) = std::fs::metadata(&target) {
            std::fs::set_permissions(&temporary, existing.permissions())?;
        }
        std::fs::rename(&temporary, &target)
    };
    write().inspect_err(|_| {
        std::fs::remove_file(&temporary).ok();
    })
}

/// What `path` names once its symbolic links are followed.
///
/// `canonicalize` answers for a link pointing at a file that exists. A link
/// left pointing at one that does not — a dotfiles repository whose
/// `config.toml` has not been made yet — has to be followed a hop at a time
/// instead, or the rename would land on the link and replace it with a
/// regular file. A path that is no link at all, existing or not, is its own
/// answer.
fn resolve_target(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let mut resolved = path.to_path_buf();
    // A bound rather than a `while`, so a link pointing at itself ends the
    // walk instead of the process.
    for _ in 0..MAX_SYMLINK_HOPS {
        let Ok(next) = std::fs::read_link(&resolved) else {
            return resolved;
        };
        resolved = if next.is_absolute() {
            next
        } else {
            resolved.parent().unwrap_or(Path::new(".")).join(next)
        };
    }
    resolved
}

/// Add the rows to `document`, reporting `Ok(false)` when a layout is already
/// set and nothing should be written.
fn insert_rows(document: &mut DocumentMut) -> Result<bool, String> {
    if rows_are_set(document) {
        return Ok(false);
    }

    // A file with something in it already gets a blank line above the new
    // table; an empty one does not want to open with one.
    let separate = !document.to_string().trim().is_empty();

    let mut table = document.as_table_mut();
    let mut is_new = false;
    for depth in 0..TABLE_PATH.len() {
        let key = TABLE_PATH[depth];
        is_new = !table.contains_key(key);
        let entry = table.entry(key).or_insert_with(|| {
            let mut created = Table::new();
            // An implicit table prints no header of its own, so the layout
            // arrives as one `[ui.sidebar.spaces]` rather than as two empty
            // parents above it.
            created.set_implicit(true);
            Item::Table(created)
        });
        table = entry
            .as_table_mut()
            .ok_or_else(|| format!("`{}` is not a table", TABLE_PATH[..=depth].join(".")))?;
    }

    // The table the rows land in is the one worth naming, so it keeps its
    // header even when this call is what created it. A table that was already
    // there keeps its own decoration: the comment above someone's
    // `[ui.sidebar.spaces]` is theirs, not ours to replace.
    table.set_implicit(false);
    if is_new && separate {
        table.decor_mut().set_prefix("\n");
    }
    let (key, rows) = seeded_rows();
    table.insert_formatted(&key, rows);
    Ok(true)
}

/// Whether `ui.sidebar.spaces.rows` already has a value.
///
/// The walk goes through anything table shaped, so a layout written as an
/// inline table counts as set just as a `[ui.sidebar.spaces]` header does.
fn rows_are_set(document: &DocumentMut) -> bool {
    let mut item = document.as_item();
    for key in TABLE_PATH.iter().copied().chain([ROWS_KEY]) {
        let Some(next) = item.as_table_like().and_then(|table| table.get(key)) else {
            return false;
        };
        item = next;
    }
    !item.is_none()
}

/// The `rows` entry out of the seed file, comment and line breaks intact.
///
/// The key comes along with the value because the comment above the rows is
/// decoration on the key, and that comment is the part that tells whoever
/// opens the file later where the block came from.
fn seeded_rows() -> (Key, Item) {
    let seed: DocumentMut = SPACES_ROWS.parse().expect("the seeded rows are valid TOML");
    let (key, rows) = seed
        .as_table()
        .get_key_value(ROWS_KEY)
        .expect("the seed sets rows");
    (key.clone(), rows.clone())
}

#[cfg(test)]
mod tests {
    use super::{resolve_config_path, write_rows, Outcome, HERDR_DIR_NAME, SPACES_ROWS};
    use std::path::{Path, PathBuf};

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "herdr-mem-cpu-load-sidebar-test-{}-{name}",
            std::process::id()
        ));
        path
    }

    /// Write `text` to a fresh file, seed it, and hand back what it became.
    fn seed(name: &str, text: &str) -> (PathBuf, Outcome, String) {
        let path = temp_path(name);
        if !text.is_empty() {
            std::fs::write(&path, text).expect("write");
        }
        let outcome = write_rows(&path).expect("the rows are seeded");
        let written = std::fs::read_to_string(&path).expect("read back");
        std::fs::remove_file(&path).ok();
        (path, outcome, written)
    }

    /// The rows as herdr parses them: a list of rows of tokens.
    fn rows(text: &str) -> Vec<toml::Value> {
        let value: toml::Value = toml::from_str(text).expect("the result is valid TOML");
        value["ui"]["sidebar"]["spaces"]["rows"]
            .as_array()
            .expect("rows is an array")
            .clone()
    }

    #[test]
    fn a_missing_file_is_written_from_scratch() {
        let (_, outcome, written) = seed("missing.toml", "");
        assert_eq!(outcome, Outcome::Written);
        assert!(
            written.contains("[ui.sidebar.spaces]"),
            "the table is named once: {written}"
        );
        // The two herdr shows by default, then one row per metric.
        assert_eq!(rows(&written).len(), 5);
        // The comment explaining where the rows came from rides along.
        assert!(
            written.contains("# Added by herdr-mem-cpu-load"),
            "{written}"
        );
    }

    #[test]
    fn an_existing_file_keeps_everything_it_had() {
        let existing = "# my herdr\n[ui]\nsidebar_width = 30\n\n[theme]\nname = \"mocha\"\n";
        let (_, outcome, written) = seed("existing.toml", existing);

        assert_eq!(outcome, Outcome::Written);
        assert!(written.starts_with("# my herdr\n"), "{written}");
        assert!(written.contains("sidebar_width = 30"), "{written}");
        assert!(written.contains("name = \"mocha\""), "{written}");
        assert_eq!(rows(&written).len(), 5);
        // The parent tables the walk created stay implicit: `[ui]` was
        // already there, and a bare `[ui.sidebar]` header is not added.
        assert!(!written.contains("[ui.sidebar]\n"), "{written}");
    }

    #[test]
    fn a_layout_that_is_already_set_is_left_alone() {
        let existing = "[ui.sidebar.spaces]\nrows = [[\"workspace\"]]\n";
        let (_, outcome, written) = seed("already-set.toml", existing);

        assert_eq!(outcome, Outcome::AlreadySet);
        assert_eq!(written, existing);
    }

    #[test]
    fn an_inline_layout_counts_as_set_too() {
        let existing = "ui = { sidebar = { spaces = { rows = [[\"workspace\"]] } } }\n";
        let (_, outcome, written) = seed("inline-set.toml", existing);

        assert_eq!(outcome, Outcome::AlreadySet);
        assert_eq!(written, existing);
    }

    #[test]
    fn a_spaces_table_without_rows_is_filled_in() {
        let existing = "[ui.sidebar.spaces]\nrow_gap = 1\n";
        let (_, outcome, written) = seed("no-rows.toml", existing);

        assert_eq!(outcome, Outcome::Written);
        assert!(written.contains("row_gap = 1"), "{written}");
        assert_eq!(rows(&written).len(), 5);
        // One table, not a second header for the same path.
        assert_eq!(
            written.matches("[ui.sidebar.spaces]").count(),
            1,
            "{written}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_config_is_written_where_it_really_lives() {
        // A `config.toml` symlinked out of a dotfiles repository is the
        // common case, and replacing the link with a regular file would take
        // it out of that repository.
        let real = temp_path("dotfiles-config.toml");
        let link = temp_path("linked-config.toml");
        std::fs::write(&real, "[theme]\nname = \"mocha\"\n").expect("write");
        std::fs::remove_file(&link).ok();
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let outcome = write_rows(&link).expect("the rows are seeded");
        assert_eq!(outcome, Outcome::Written);
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("the link is still there")
                .file_type()
                .is_symlink(),
            "the link was replaced by a regular file"
        );
        let written = std::fs::read_to_string(&real).expect("read the file it points at");
        assert!(written.contains("name = \"mocha\""), "{written}");
        assert_eq!(rows(&written).len(), 5, "{written}");

        std::fs::remove_file(&link).ok();
        std::fs::remove_file(&real).ok();
    }

    #[test]
    #[cfg(unix)]
    fn a_link_to_a_file_that_is_not_there_yet_is_followed_rather_than_replaced() {
        let real = temp_path("dangling-target.toml");
        let link = temp_path("dangling-config.toml");
        std::fs::remove_file(&real).ok();
        std::fs::remove_file(&link).ok();
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let outcome = write_rows(&link).expect("the rows are seeded");
        assert_eq!(outcome, Outcome::Written);
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("the link is still there")
                .file_type()
                .is_symlink(),
            "the link was replaced by a regular file"
        );
        // The file it points at is the one that was created.
        assert_eq!(
            rows(&std::fs::read_to_string(&real).expect("the target was written")).len(),
            5
        );

        std::fs::remove_file(&link).ok();
        std::fs::remove_file(&real).ok();
    }

    #[test]
    #[cfg(unix)]
    fn the_permissions_a_config_had_are_the_permissions_it_keeps() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("private-config.toml");
        std::fs::write(&path, "[theme]\nname = \"mocha\"\n").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");

        write_rows(&path).expect("the rows are seeded");

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the file came back world readable");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_file_that_does_not_parse_is_reported_and_left_alone() {
        let path = temp_path("broken.toml");
        let broken = "[ui\n";
        std::fs::write(&path, broken).expect("write");

        let error = write_rows(&path).expect_err("a broken file is an error");
        assert_eq!(error.path, path);
        assert!(error.to_string().contains("leaving it alone"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), broken);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_ui_key_that_is_not_a_table_is_an_error_rather_than_a_clobbering() {
        let path = temp_path("ui-scalar.toml");
        let existing = "ui = 4\n";
        std::fs::write(&path, existing).expect("write");

        let error = write_rows(&path).expect_err("a scalar `ui` is an error");
        assert!(error.to_string().contains("`ui` is not a table"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), existing);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_seeded_rows_name_every_level_token_the_daemon_reports() {
        // Each metric lists all three levels, because the daemon sets one and
        // clears the other two.
        for token in [
            "$cpu_ok",
            "$cpu_warn",
            "$cpu_hot",
            "$mem_ok",
            "$mem_warn",
            "$mem_hot",
            "$load_ok",
            "$load_warn",
            "$load_hot",
        ] {
            assert!(
                SPACES_ROWS.contains(token),
                "the seeded rows are missing {token}"
            );
        }
        // herdr caps a layout at sixteen rows of sixteen tokens.
        let seeded: toml::Value = toml::from_str(SPACES_ROWS).expect("the seed is valid TOML");
        let seeded = seeded["rows"].as_array().expect("rows is an array");
        assert!(seeded.len() <= 16);
        assert!(seeded
            .iter()
            .all(|row| row.as_array().is_some_and(|row| row.len() <= 16)));
    }

    // Which environment variable feeds which argument is covered end to end
    // in tests/cli.rs, where a child process can be given an environment of
    // its own: setting one from a test here would race every other test in
    // the binary.
    #[test]
    fn the_path_follows_herdr_from_the_explicit_setting_down_to_the_platform() {
        let explicit = PathBuf::from("/somewhere/herdr.toml");
        assert_eq!(
            resolve_config_path(Some(explicit.clone()), None, None),
            explicit,
            "an explicit path wins outright"
        );
        assert_eq!(
            resolve_config_path(None, Some(PathBuf::from("/xdg")), None),
            Path::new("/xdg").join(HERDR_DIR_NAME).join("config.toml"),
            "herdr hangs its own directory under the configuration home"
        );
        assert_eq!(
            resolve_config_path(None, None, Some(PathBuf::from("/home/me/.config/herdr"))),
            Path::new("/home/me/.config/herdr/config.toml"),
            "the platform directory already names herdr"
        );
        // Nothing set at all still resolves to a path rather than panicking.
        assert!(resolve_config_path(None, None, None).ends_with("config.toml"));
    }
}
