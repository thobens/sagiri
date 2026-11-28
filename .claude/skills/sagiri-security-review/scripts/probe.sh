#!/usr/bin/env bash
# Runs probe text through the `sagiri` filter and shows, line by line, what was kept and
# what was changed.
#
# usage: probe.sh [options] [FILE]
#
#   FILE           the probe text; stdin if missing. Fictional data only.
#   --corpus       use the corpus of realistic paste formats next to this script
#                  (corpus.txt, comment lines removed) instead of FILE or stdin
#   --config FILE  a config file for this run (allow, terms, rules). Default: empty.
#   --only KIND    switch every rule off except KIND (term, url, email, ipv4, ipv6,
#                  hostname, payment_card, iban, phone), to see which rule replaces a
#                  value. Not together with --config.
#   --kept         print only the lines that came out unchanged
#   --summary      print only the counts of kept and changed lines
#   --release      use a release build and print the time the run took
#   --root DIR     probe another checkout, for example the worktree of a ticket branch
#
# The run is isolated from the real user data:
#   - the key is the published test key of the golden vectors (bytes 00 01 .. 1f), so a
#     probe gives the same output on every machine and can be quoted in a report;
#   - the config is a copy of the given file or an empty one, never
#     ~/.config/sagiri/config.toml. Error messages name the copy;
#   - the environment is emptied, and HOME and the XDG directories point into a temporary
#     directory that is removed afterwards.
# No clipboard, display or session process is touched.
#
# Output:
#   kept     | a line that came out unchanged
#   changed  | a line as it went in
#         -> | the same line as it came out
# followed by the counts, the exit status, anything written to stderr, and every file the
# run left behind besides the key and the config. A line count that differs between input
# and output is reported as well: Sagiri promises that it never happens.
#
# Invisible characters are printed as they are; pipe the output through `cat -A` to see
# them.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root=""
config=""
input=""
only=""
corpus=0
kept_only=0
summary=0
profile=debug
while [ "$#" -gt 0 ]; do
    case "$1" in
        --root)
            shift
            root="${1:?--root needs a directory}"
            ;;
        --config)
            shift
            config="${1:?--config needs a file}"
            ;;
        --only)
            shift
            only="${1:?--only needs a kind}"
            ;;
        --corpus) corpus=1 ;;
        --kept) kept_only=1 ;;
        --summary) summary=1 ;;
        --release) profile=release ;;
        -h | --help)
            sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        -*)
            echo "probe.sh: unknown option: $1" >&2
            exit 2
            ;;
        *) input="$1" ;;
    esac
    shift
done
kinds="term url email ipv4 ipv6 hostname payment_card iban phone"
if [ -n "$only" ]; then
    if [ -n "$config" ]; then
        echo "probe.sh: --only and --config cannot be combined" >&2
        exit 2
    fi
    case " $kinds " in
        *" $only "*) ;;
        *)
            echo "probe.sh: unknown kind: $only (one of: $kinds)" >&2
            exit 2
            ;;
    esac
fi
if [ -z "$root" ]; then
    root="$(git -C "$here" rev-parse --show-toplevel)"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/home"

if [ "$corpus" -eq 1 ]; then
    grep -v '^#' "$here/corpus.txt" >"$tmp/in"
elif [ -n "$input" ]; then
    cat -- "$input" >"$tmp/in"
else
    cat >"$tmp/in"
fi
if [ -n "$config" ]; then
    cat -- "$config" >"$tmp/home/config.toml"
elif [ -n "$only" ]; then
    {
        echo "[rules]"
        for kind in $kinds; do
            [ "$kind" = "$only" ] || echo "$kind = false"
        done
    } >"$tmp/home/config.toml"
else
    : >"$tmp/home/config.toml"
fi
(
    umask 077
    printf '000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\n' >"$tmp/home/key"
)

cd "$root"
if [ "$profile" = release ]; then
    cargo build --quiet --locked --release -p sagiri --bin sagiri
else
    cargo build --quiet --locked -p sagiri --bin sagiri
fi
target_dir="$(cargo metadata --format-version 1 --no-deps --locked |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
bin="$target_dir/$profile/sagiri"

status=0
started="$(date +%s.%N)"
env -i HOME="$tmp/home" XDG_CONFIG_HOME="$tmp/home/xdg-config" \
    XDG_DATA_HOME="$tmp/home/xdg-data" XDG_RUNTIME_DIR="$tmp/home/xdg-run" \
    "$bin" --key-file "$tmp/home/key" --config "$tmp/home/config.toml" \
    <"$tmp/in" >"$tmp/out" 2>"$tmp/err" || status=$?
finished="$(date +%s.%N)"

lines_in="$(awk 'END { print NR }' "$tmp/in")"
lines_out="$(awk 'END { print NR }' "$tmp/out")"
if [ "$status" -eq 0 ] && [ "$lines_in" -eq "$lines_out" ]; then
    # The empty strings force a comparison as text: awk would otherwise call `010` and
    # `10` equal.
    awk -v out="$tmp/out" -v kept_only="$kept_only" -v summary="$summary" '
        {
            if ((getline o < out) <= 0) o = ""
            if (($0 "") == (o "")) {
                kept++
                if (!summary) printf "kept     | %s\n", $0
            } else {
                changed++
                if (!summary && !kept_only) printf "changed  | %s\n      -> | %s\n", $0, o
            }
        }
        END { printf "# %d line(s) kept, %d changed\n", kept, changed }' "$tmp/in"
else
    echo "# no line-by-line comparison: exit status $status, $lines_in line(s) in, $lines_out line(s) out"
    echo "# stdout:"
    cat "$tmp/out"
fi

echo "# exit status: $status"
if [ "$profile" = release ]; then
    awk -v a="$started" -v b="$finished" 'BEGIN { printf "# time: %.2f s (release build)\n", b - a }'
fi
if [ -s "$tmp/err" ]; then
    echo "# stderr:"
    sed 's/^/#   /' "$tmp/err"
fi
left="$(cd "$tmp/home" && find . -mindepth 1 ! -path ./key ! -path ./config.toml | LC_ALL=C sort)"
if [ -n "$left" ]; then
    echo "# files and directories the run left behind:"
    printf '%s\n' "$left" | sed 's/^/#   /'
fi
