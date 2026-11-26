#!/usr/bin/env bash
# Checks that a tag may be released. Used by .github/workflows/release.yml.
#
# Usage: scripts/release-tag-check.sh <tag> [<commit>]
#
# - <tag> must be `v` followed by the workspace version from `cargo metadata`.
# - <commit> (default HEAD, the tagged commit in the workflow) must be an ancestor of
#   origin/master.
#
# Exit status:
#   0  the tag may be released
#   1  the tag does not match the version, or the commit is not on origin/master
#   2  wrong usage (including a <commit> or origin/master that does not exist), or the
#      version could not be read
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ] || [ -z "$1" ]; then
    echo "usage: scripts/release-tag-check.sh <tag> [<commit>]" >&2
    exit 2
fi
tag="$1"
commit="${2:-HEAD}"

cd "$(dirname "$0")/.." || exit 2

if ! version="$(cargo metadata --format-version 1 --no-deps --locked |
    jq -r '.packages[] | select(.name == "sagiri") | .version')" || [ -z "$version" ]; then
    echo "release-tag-check.sh: could not read the workspace version" >&2
    exit 2
fi

if [ "$tag" != "v$version" ]; then
    echo "release-tag-check.sh: tag $tag does not match the workspace version $version (expected v$version)" >&2
    exit 1
fi

for ref in "$commit" origin/master; do
    if ! git rev-parse --verify --quiet "$ref^{commit}" >/dev/null; then
        echo "release-tag-check.sh: $ref does not name a commit" >&2
        exit 2
    fi
done

if ! git merge-base --is-ancestor "$commit" origin/master; then
    echo "release-tag-check.sh: $commit is not an ancestor of origin/master" >&2
    exit 1
fi

echo "release-tag-check.sh: $tag matches the workspace version and is on origin/master"
