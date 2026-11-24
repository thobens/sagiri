---
id: "0020"
title: Control channel and sagiri-session
status: todo
depends_on: ["0018", "0019"]
---

# 0020 — Control channel and `sagiri-session`

## Why

A session is switched on and off from outside: by a key binding of the window manager, by
a script, later by Sagiri's own hotkey. That needs a process that keeps running and a way
to talk to it. This ticket adds the `sagiri-session` binary: `run` is the long-running
process, and `start`, `stop`, `toggle`, `status` and `quit` are small clients that reach it
through a Unix socket. There is no display backend yet, so `run` cannot start a real session
before ticket 0021; everything else is complete and tested.

## Scope

In `crates/sagiri`. Add `signal-hook` 0.3 (see `tickets/README.md`, "Crates").

### Command line (`src/bin/sagiri-session.rs`)

```
sagiri-session [--socket <PATH>] <COMMAND>

  run [--active] [--key-file <PATH>] [--config <PATH>]
  start | stop | toggle | status | quit
```

- A command is required; without one clap prints the usage and exits 2.
- Socket path: `--socket`, otherwise `sagiri_core::paths::runtime_dir()` joined with
  `session.sock`.
- Client commands print the reply (`active` or `inactive`) on stdout and exit 0.
- Messages go to stderr with the prefix `sagiri-session: `; exit code 1:

  | Situation | Message |
  |---|---|
  | client, nothing listens on the socket | `no session process is running; start one with 'sagiri-session run'` |
  | no `--socket` and no usable `XDG_RUNTIME_DIR` | `XDG_RUNTIME_DIR is not set to an absolute path; use --socket <PATH>` |
  | `run`, another instance listens | `a session process is already running` |
  | `run`, no backend (always, until ticket 0021) | `session mode has no display backend on this platform` |
  | the server answered `error <reason>` | `<reason>` |
  | key or config error | as in `sagiri` |

- `run` exits 0 after `quit`, SIGINT or SIGTERM, and 1 after `Exit::BackendLost`
  (`pub fn exit_code(exit: Exit) -> u8` in `daemon.rs`).

### Protocol and socket (`src/session/control.rs`)

The protocol is pinned in `tickets/README.md`.

- `pub fn parse(line: &str) -> Option<Command>` and
  `pub fn reply_line(active: bool) -> &'static str`.
- Server:
  - `Listener::bind(path)`:
    - creates the parent directory with mode 0700 if it is missing;
    - if the path exists and a connection to it succeeds, fails with "already running";
    - if the path exists and the connection is refused, removes the stale file and binds;
    - sets the socket file's mode to 0600.
  - Dropping the `Listener` removes the socket file.
  - `serve(listener, events)`: for each connection, read one line of at most 64 bytes with
    a one-second timeout, send `Event::Command` with a reply channel, wait up to five
    seconds for the state, answer, close.
  - Fixed error replies: `error unknown command`, `error request too long`,
    `error timeout`.
- Client: `pub fn request(path: &Path, command: Command) -> Result<bool, RequestError>`,
  where `RequestError::NotRunning` covers a missing socket file and a refused connection.

### Wiring (`src/session/daemon.rs`, `backend.rs`, `notify.rs`)

- `pub struct Parts { pub session: Session, pub socket: PathBuf, pub start_active: bool,
  pub sources: Box<dyn FnOnce(Sender<Event>) -> anyhow::Result<()>> }`.
- `pub fn run(parts: Parts) -> anyhow::Result<Exit>`:
  1. bind the listener;
  2. create the event channel and call `sources` with a sender. `sources` starts every
     event source other than the control socket. An error from it ends `run` before
     anything else happens;
  3. start the control thread;
  4. `set_active(true)` if `start_active`;
  5. `run_loop`; afterwards the socket file is gone.
- `backend.rs`: `pub fn start(events: Sender<Event>, settings: &SessionConfig)
  -> anyhow::Result<()>`. In this ticket it always returns the "no display backend" error.
- The binary's `sources` closure installs the signal thread (`SIGINT` and `SIGTERM` send
  `Event::Shutdown`) and calls `backend::start`.
- `notify.rs`: `StderrNotifier` writes one fixed line per notice to stderr, for example
  `sagiri-session: session started`.
- Order in `run`: resolve the socket path, load key and config (`setup::load`), build the
  `Session` with `SystemClipboard` and `StderrNotifier`, then `daemon::run`.

## Out of scope

- Watching the clipboard, X11 (ticket 0021). A started session receives no change events
  yet.
- The hotkey (ticket 0023) and desktop notifications (ticket 0022).
- Starting the long-running process from a client command; a systemd unit (backlog).
- The user-facing README: session mode is documented by ticket 0021, when it works.
- Authentication beyond file permissions.

## Design notes

- **A socket, not signals, for commands.** `status` needs an answer, and `toggle` should
  report the new state so that a key binding can show it.
- **Single instance by probing.** `File::try_lock` is not stable in Rust 1.88, the
  workspace's minimum version. Connecting to an existing socket tells a live instance from
  a stale file.
- **Signal handling lives in the binary.** `daemon::run` installs no handlers, so tests can
  run it in-process.
- **Testing `daemon::run`.** The fakes are not `Send`. The test runs `daemon::run` on its
  own thread and the client calls (`control::request`) on a second thread; after `quit`
  the test inspects the fakes. `sources` is a closure that does nothing.
- **The socket carries no content.** Requests are one of five words; replies are states or
  fixed texts. A request that is not one of the five words is not echoed.
- **Five seconds for a reply.** Reading the clipboard can block for several seconds when
  the owning application is slow, and `start` reads it.
- **Socket path length.** Unix socket paths are limited to about 100 bytes. Tests use a
  short file name inside a `tempfile` directory.
- `signal-hook`'s iterator API needs no `unsafe`. Check the 0.3 documentation for the
  exact names.

## Acceptance criteria

Unit tests in `control.rs`:

- [ ] `command_words_roundtrip`: a table over the five words; `reply_line` for both states.
- [ ] `unknown_word_is_none`, including an empty line and a word with trailing text.
- [ ] `unknown_request_gets_error_reply`: the reply is `error unknown command` and does
      not contain the request.
- [ ] `oversized_request_gets_error_reply`.
- [ ] `silent_client_times_out`: a connection that sends nothing does not block the next
      client for more than about one second.
- [ ] `second_instance_is_refused`.
- [ ] `stale_socket_is_replaced`.
- [ ] `socket_directory_is_created_0700` and `socket_file_is_0600`.
- [ ] `dropping_the_listener_removes_the_socket`.
- [ ] `request_without_listener_is_not_running`: for a missing file and for a stale file.

Unit tests in `daemon.rs` (fakes, a socket in a temporary directory):

- [ ] `toggle_over_socket_changes_state`: `toggle` answers `true`, `status` answers
      `true`, `toggle` answers `false`.
- [ ] `start_processes_the_clipboard`: text on the fake clipboard is obfuscated after
      `start`.
- [ ] `start_active_flag`: with `start_active`, the first `status` answers `true`.
- [ ] `quit_ends_run_and_removes_socket`.
- [ ] `sources_error_ends_run_early`: no socket file is left.
- [ ] `exit_codes`: `Quit` and `Shutdown` map to 0, `BackendLost` to 1.

Integration tests in `crates/sagiri/tests/session.rs`:

- [ ] `no_command_exits_2`.
- [ ] `client_without_daemon_exits_1`: `status --socket <tmp>/s.sock` prints the "no
      session process" message and nothing on stdout.
- [ ] `missing_runtime_dir_exits_1`: `status` without `--socket` and without
      `XDG_RUNTIME_DIR` in the child's environment.
- [ ] `run_without_backend_exits_1`: `run --socket <tmp>/s.sock` with temporary key and
      config locations; afterwards no socket file exists.

And:

- [ ] `cargo tree -p sagiri -i signal-hook` shows one version.
- [ ] `scripts/check.sh` passes, on Linux and on macOS (CI).

## Test plan

- Unit tests in `control.rs` and `daemon.rs`; integration tests in `tests/session.rs`.
- No manual checks: nothing here needs a display.
