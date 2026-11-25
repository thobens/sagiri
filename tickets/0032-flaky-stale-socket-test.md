---
id: "0032"
title: Make `stale_socket_is_replaced` independent of tests that start processes
status: done
depends_on: ["0022"]
---

# 0032 — Make `stale_socket_is_replaced` independent of tests that start processes

## Why

The unit test `session::control::tests::stale_socket_is_replaced`
(`crates/sagiri/src/session/control.rs`) fails about once in 100 runs of the library test
binary. `scripts/check.sh` and CI run that binary, so roughly one run in a hundred fails
for a reason that has nothing to do with the change under test.

The test builds a stale socket by binding a `UnixListener` and dropping it, then asserts
that connecting to the path fails:

```rust
drop(UnixListener::bind(&path).unwrap());
assert!(path.exists());
assert!(UnixStream::connect(&path).is_err());   // fails now and then
```

Other tests in the same binary start child processes (`holder.rs`, and since ticket 0022
`session/notify.rs`). Tests run on parallel threads. If one of them forks between the
`bind` and the `drop`, the child holds a copy of the listening socket until its `exec`
closes it, and for that moment `connect` succeeds.

Observed while implementing and reviewing ticket 0022:

- implementer: 1 failure in 150 runs with the two new process-starting tests of ticket 0022
  skipped, and about 1 in 100 with them;
- reviewer: 1 failure in 100 runs on `ticket/0022` at `82064d9`, at the `connect` assertion
  quoted above.

The cause above is the reading of both; nobody has proven it, for example by forcing the
interleaving. Confirming it is the first step of this ticket.

`Listener::bind` itself is not known to be wrong. Its stale-socket check makes the same
`connect` call, so the same race could in principle make a real `sagiri-session run` report
"a session process is already running" for a stale socket. Whether that can happen outside
the test binary is part of what this ticket establishes.

## Scope

- Confirm or refute the cause, and write the result into the commit message.
- Change the test, not the product behaviour, so that it no longer depends on what other
  tests do at the same moment. The test keeps proving what it proves today: a socket file
  that nothing listens on is replaced, and the new listener accepts connections.
- If the investigation shows that `Listener::bind` can misjudge a stale socket in the real
  program, stop and report it. That is a product bug and needs its own ticket.

## Out of scope

- Changes to `Listener::bind`, the control protocol or any message.
- Running the test suite single-threaded, in `scripts/check.sh` or in CI.
- Changes to the tests in `holder.rs` and `session/notify.rs`, beyond what a shared test
  helper requires.

## Design notes

- **Ways to remove the dependency.** A lock shared by the tests that start processes and
  this test; or building the stale socket without a listener of this process ever being
  open while another thread can fork, for example in a child process that exits. The choice
  is the implementer's.
- **A retry loop around the assertion is not a fix.** It hides the race and can hide a real
  regression of the stale-socket check.
- **The descriptors are close-on-exec.** The standard library opens sockets with
  `SOCK_CLOEXEC`, which is why the window is only the time between `fork` and `exec`.

## Acceptance criteria

- [ ] `stale_socket_is_replaced` still asserts that the stale socket file exists, that
      `Listener::bind` succeeds on it, and that a connection to the new listener succeeds.
- [ ] 1000 consecutive runs of the library test binary of `crates/sagiri` show no failure.
      The command and its result are in the implementer's report.
- [ ] `scripts/check.sh` passes, and its test run is not single-threaded.

## Test plan

- A shell loop over the built test binary (`target/debug/deps/sagiri-*`), counting
  failures, before and after the change.
