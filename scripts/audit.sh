#!/usr/bin/env bash
# Checks compiled binaries against the RustSec advisory database with `cargo audit bin`.
# Needs network access. Not part of scripts/check.sh.
#
# Without arguments: builds sagiri, sagiri-cli and sagiri-session in release mode with
# `cargo auditable` (through the Makefile) and audits them.
# With arguments: audits the binaries at these paths and builds nothing.
#
# Exit status:
#   0  no binary has a known vulnerability
#   1  at least one binary has a known vulnerability
#   2  cargo-audit or cargo-auditable is not installed, or an argument is not a file
#   3  the audit is incomplete: a binary has no embedded dependency data, or
#      `cargo audit` failed for another reason (for example the advisory database could
#      not be fetched)
# Warnings (unmaintained, yanked and similar) are printed and do not change the status.
set -uo pipefail

bins=(sagiri sagiri-cli sagiri-session)

# Relative paths are made absolute before the directory changes.
paths=()
for arg in "$@"; do
    case "$arg" in
        /*) path="$arg" ;;
        *) path="$PWD/$arg" ;;
    esac
    if [ ! -f "$path" ]; then
        echo "audit.sh: $arg is not a file" >&2
        exit 2
    fi
    paths+=("$path")
done

cd "$(dirname "$0")/.." || exit 2

required=(cargo-audit)
if [ "${#paths[@]}" -eq 0 ]; then
    required+=(cargo-auditable)
fi
for program in "${required[@]}"; do
    if ! command -v "$program" >/dev/null 2>&1; then
        echo "audit.sh: $program is not installed" >&2
        exit 2
    fi
done

if [ "${#paths[@]}" -eq 0 ]; then
    make release CARGO='cargo auditable' || exit 3
    release_dir="${CARGO_TARGET_DIR:-target}/release"
    for bin in "${bins[@]}"; do
        paths+=("$release_dir/$bin")
    done
fi

log="$(mktemp)" || exit 3
trap 'rm -f "$log"' EXIT

# `cargo audit bin` exits 0 for a binary without embedded data and only prints a warning,
# and its JSON output does not tell the two cases apart. The script therefore requires the
# message that it prints for a complete scan, once per binary.
cargo audit --color never bin -- "${paths[@]}" 2>&1 | tee "$log"
audit_status="${PIPESTATUS[0]}"

incomplete=0
for path in "${paths[@]}"; do
    if ! grep -qF "Found 'cargo auditable' data in $path (" "$log"; then
        echo "audit.sh: $path has no dependency data from cargo auditable; the check is incomplete" >&2
        incomplete=1
    fi
done

case "$audit_status" in
    0) ;;
    1)
        echo "audit.sh: at least one binary has a known vulnerability" >&2
        exit 1
        ;;
    *)
        echo "audit.sh: cargo audit failed with status $audit_status" >&2
        exit 3
        ;;
esac

if [ "$incomplete" -ne 0 ]; then
    exit 3
fi
echo "audit.sh: no known vulnerability in ${paths[*]}"
