#!/usr/bin/env bash
# Runs the X11 integration tests of session mode under a virtual X server (Xvfb).
# Opt-in: not part of scripts/check.sh or CI.
set -uo pipefail
cd "$(dirname "$0")/.."

xvfb_run="${XVFB_RUN:-xvfb-run}"
for program in "$xvfb_run" xdotool; do
    if ! command -v "$program" >/dev/null 2>&1; then
        echo "x11-test.sh: $program is not installed" >&2
        exit 2
    fi
done

# The tests must talk to Xvfb only; a Wayland display would take precedence.
unset WAYLAND_DISPLAY

"$xvfb_run" -a cargo test -p sagiri --locked --test session_x11 -- --ignored --test-threads=1
