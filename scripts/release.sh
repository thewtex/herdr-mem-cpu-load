#!/bin/sh
#
# Cut a release: bump the three version numbers together, close out the
# changelog, commit, and tag.
#
#   scripts/release.sh 0.2.0
#   scripts/release.sh --dry-run 0.2.0
#
# The version lives in three places — `Cargo.toml`, `Cargo.lock`, and
# `herdr-plugin.toml` — and herdr's marketplace reads the last one while
# `cargo` reads the first. Bumping them by hand is how they drift, so this is
# the only supported way to do it; CI fails the build when they disagree.
#
# Nothing is pushed. The tag and the commit stay local until you push them.

set -eu

usage() {
    cat >&2 <<'USAGE'
usage: scripts/release.sh [--dry-run] <version>

  <version>   the new version, without a leading `v` (e.g. 0.2.0)
  --dry-run   show what would change and leave the worktree alone
USAGE
    exit 2
}

die() {
    echo "release: $*" >&2
    exit 1
}

DRY_RUN=0
VERSION=""
while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) DRY_RUN=1 ;;
        -h | --help) usage ;;
        -*) die "unknown option $1" ;;
        *)
            [ -z "$VERSION" ] || usage
            VERSION="$1"
            ;;
    esac
    shift
done
[ -n "$VERSION" ] || usage

VERSION="${VERSION#v}"
case "$VERSION" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) die "'$VERSION' is not a semantic version like 0.2.0" ;;
esac

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

TAG="v$VERSION"
TODAY=$(date -u +%Y-%m-%d)

command -v python3 >/dev/null 2>&1 || die "python3 is required"
git rev-parse --git-dir >/dev/null 2>&1 || die "not a git repository"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && die "$TAG already exists"

if [ "$DRY_RUN" -eq 0 ] && [ -n "$(git status --porcelain)" ]; then
    die "the worktree is dirty; commit or stash first"
fi

# Rewrite the first line-initial `version = ` of a TOML file. Dependency
# versions are inline (`clap = { version = "4" }`), so they never match.
bump_toml() {
    file=$1
    awk -v version="$VERSION" '
        !done && /^version = / { print "version = \"" version "\""; done = 1; next }
        { print }
    ' "$file" >"$file.release-tmp"
    mv "$file.release-tmp" "$file"
}

# Turn the open `## [Unreleased]` heading into this release, and open a fresh
# empty one above it.
close_changelog() {
    awk -v version="$VERSION" -v today="$TODAY" '
        !done && /^## \[Unreleased\]/ {
            print "## [Unreleased]"
            print ""
            print "## [" version "] - " today
            done = 1
            next
        }
        { print }
        END { if (!done) { print "release: CHANGELOG.md has no ## [Unreleased] heading" > "/dev/stderr"; exit 1 } }
    ' CHANGELOG.md >CHANGELOG.md.release-tmp
    mv CHANGELOG.md.release-tmp CHANGELOG.md
}

CURRENT=$(awk '/^version = /{ gsub(/[",]/, "", $3); print $3; exit }' Cargo.toml)
echo "release: $CURRENT -> $VERSION ($TAG, $TODAY)"

if [ "$DRY_RUN" -eq 1 ]; then
    echo "release: --dry-run, nothing written"
    exit 0
fi

bump_toml Cargo.toml
bump_toml herdr-plugin.toml
close_changelog

# Any resolving cargo command rewrites the version of the local package in
# Cargo.lock; metadata is the cheapest one.
cargo metadata --format-version 1 --offline >/dev/null 2>&1 ||
    cargo metadata --format-version 1 >/dev/null

python3 scripts/check_manifest.py

git add Cargo.toml Cargo.lock herdr-plugin.toml CHANGELOG.md
git commit -m "chore: release $TAG"
git tag -a "$TAG" -m "$TAG"

cat <<DONE

release: committed and tagged $TAG. Nothing has been pushed.

  git push origin HEAD
  git push origin $TAG

Pushing the tag is what runs .github/workflows/release.yml.
DONE
