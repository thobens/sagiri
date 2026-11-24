---
id: "0026"
title: Xvfb integration script
status: todo
depends_on: ["0023"]
---

# 0026 — Xvfb integration script

## Why

The X11 parts of session mode (the watcher of ticket 0021 and the key grab of ticket 0023)
are proven only by manual checklists, because the quality gate must run without a display.
A virtual X server closes that gap: `Xvfb` provides a real X server without a screen. This
ticket adds tests that drive the real `sagiri-session` binary against it, and a script that
runs them. The script is opt-in and stays out of the quality gate and of CI.

## Scope

### Script (`scripts/x11-test.sh`)

- Checks that `xvfb-run` and `xdotool` are installed. If one is missing, it prints which
  one and exits 2. The name of the first program can be overridden with the environment
  variable `XVFB_RUN` (default `xvfb-run`).
- Runs, from the repository root:

  ```
  xvfb-run -a cargo test -p sagiri --locked --test session_x11 -- --ignored --test-threads=1
  ```

- Exits with the status of that command.

### Tests (`crates/sagiri/tests/session_x11.rs`)

- The whole file is `#[cfg(target_os = "linux")]`. Every test is
  `#[ignore = "needs an X server; run scripts/x11-test.sh"]`.
- Each test starts `sagiri-session run` with a temporary key file, a temporary config file
  with `notify = false`, and `--socket` in a temporary directory. It stops the process with
  `sagiri-session quit` at the end, also when an assertion fails.
- The test process itself is the application that copies: it sets and reads the clipboard
  with `arboard`.
- Waiting is done by polling with a deadline of five seconds, never with a fixed sleep
  that decides the result.
- Tests:
  - `x11_replaces_copied_text`: start a session, copy `server 172.55.0.1 is down`; within
    the deadline the clipboard holds a different text that does not contain `172.55.0.1`.
  - `x11_same_text_twice_gets_same_replacement`.
  - `x11_does_not_loop`: 500 ms after the replacement the clipboard still holds the same
    text.
  - `x11_inactive_session_leaves_text_alone`: without `start`, the copied text is still
    there after 500 ms.
  - `x11_stop_ends_replacement`.
  - `x11_second_instance_refused`: a second `run` with the same socket exits 1.
  - `x11_hotkey_toggles`: with `hotkey = "ctrl+alt+o"` in the config,
    `xdotool key ctrl+alt+o` makes `status` print `active`, and a second one `inactive`.

### Documentation

- `README.md`, section "Checks": one paragraph on `scripts/x11-test.sh`, what it needs
  (`xvfb-run` from the X server's Xvfb package, `xdotool`) and that it is not part of
  `scripts/check.sh`.

## Out of scope

- Running the script in CI.
- Tests for the PRIMARY selection (tickets 0024 and 0025).
- Tests for desktop notifications.
- Any change to `scripts/check.sh` or `.github/`.

## Design notes

- **Why ignored tests.** `scripts/check.sh` compiles them (clippy with all targets) and
  skips them at run time. The script runs exactly these.
- **One thread.** All tests share one X server and one clipboard.
- **A fresh socket and key per test** keep the tests independent of each other and of the
  user's real session.
- **`XDG_RUNTIME_DIR` is not needed**, because `--socket` is given.
- **No window manager runs under Xvfb.** `xdotool key` sends the key through the XTEST
  extension, which is enough to trigger a grab on the root window.
- **Fixed sleeps only for "nothing happens" checks.** Proving that something does not
  change needs a waiting time; 500 ms is far above the few milliseconds a replacement
  takes.
- The sample text and all other test data are fictional, as everywhere.

## Acceptance criteria

- [ ] `scripts/x11-test.sh` is executable and exits 0 on a machine with `xvfb-run` and
      `xdotool`. The report includes the test summary line.
- [ ] `XVFB_RUN=no-such-program scripts/x11-test.sh` exits 2 and names `no-such-program`.
- [ ] `cargo test -p sagiri --test session_x11` without `--ignored` runs no test and
      passes.
- [ ] `grep -rn "x11-test" scripts/check.sh .github` prints nothing.
- [ ] After the script, `pgrep -f "sagiri-session run"` finds no process left over from
      the tests.
- [ ] `README.md` has the paragraph described above.
- [ ] `scripts/check.sh` passes.

## Test plan

- Run `scripts/x11-test.sh` three times in a row; all runs must pass (the tests wait for
  events and must not be flaky).
- Run `scripts/check.sh`.
