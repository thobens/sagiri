#!/usr/bin/env bash
# Packs the release binaries that are already built into
# dist/sagiri-<version>-<target>.tar.gz and prints that path. Builds nothing.
#
# Usage: scripts/package.sh <target>
#
# The files are staged with `make install`, so the Makefile alone defines what is
# installed where. <version> is the workspace version from `cargo metadata`.
#
# Exit status:
#   0  the archive was written
#   1  the version could not be read, or staging or packing failed
#   2  <target> is missing, or a binary is not built
set -euo pipefail

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
    echo "usage: scripts/package.sh <target>" >&2
    exit 2
fi
target="$1"

# Any other failing command (make, cp, tar, gzip) ends the script with status 1, so that
# its own status (make and tar use 2) is not mistaken for a usage error.
trap 'exit 1' ERR

cd "$(dirname "$0")/.."

bins=(sagiri sagiri-cli sagiri-session)
release_dir="${CARGO_TARGET_DIR:-target}/release"
for bin in "${bins[@]}"; do
    if [ ! -x "$release_dir/$bin" ]; then
        echo "package.sh: missing $release_dir/$bin: run 'make release' first" >&2
        exit 2
    fi
done

if ! version="$(cargo metadata --format-version 1 --no-deps --locked |
    jq -r '.packages[] | select(.name == "sagiri") | .version')" || [ -z "$version" ]; then
    echo "package.sh: could not read the workspace version" >&2
    exit 1
fi

name="sagiri-$version-$target"
archive="dist/$name.tar.gz"

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT

# USERUNITDIR is unset so that only the Makefile default applies.
env -u USERUNITDIR make --no-print-directory install \
    DESTDIR="$staging/root" PREFIX=/usr/local >&2

tree="$staging/$name"
mkdir -p "$tree"
cp -R "$staging/root/usr/local/." "$tree/"
for file in LICENSE-MIT LICENSE-APACHE README.md; do
    install -m 644 "$file" "$tree/$file"
done

# Files only, sorted bytewise, so that the listing does not depend on the machine.
list="$staging/files"
(cd "$staging" && find "$name" -type f | LC_ALL=C sort) >"$list"

if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    owner=(--owner=root:0 --group=root:0)
else
    # bsdtar (macOS)
    owner=(--uid 0 --uname root --gid 0 --gname root)
fi

mkdir -p dist
tar -c -f "$staging/archive.tar" -C "$staging" "${owner[@]}" --no-recursion -T "$list"
gzip -n -9 <"$staging/archive.tar" >"$staging/archive.tar.gz"
mv -f "$staging/archive.tar.gz" "$archive"

echo "$archive"
