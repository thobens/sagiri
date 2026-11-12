# Sagiri

Sagiri obfuscates pasted text. It finds sensitive values such as IP addresses, URLs,
hostnames, email addresses, card numbers, IBANs and phone numbers, and replaces each with a
realistic-looking fake value. The same original always gets the same replacement, across
pastes and across sessions.

Status: pre-MVP.

## Build and run

```
cargo build
cargo run -p sagiri -- --help
```

## Usage

```
sagiri < input.txt > output.txt
```

Text on stdin is written to stdout with sensitive values replaced. The same original gets
the same replacement on every run. Running `sagiri` with a terminal on stdin prints a hint
and exits with status 2 until interactive mode exists.

The secret key is stored in `$XDG_DATA_HOME/sagiri/key` (default
`~/.local/share/sagiri/key`) and is created on first use with mode 0600. `--key-file <PATH>`
or the `SAGIRI_KEY_FILE` environment variable selects another file; the flag wins.
Deleting the key file changes all replacements. Keep it private.

Exit codes: 0 success, 1 runtime error (for example input that is not valid UTF-8), 2 usage
error.

Known limitation: a bare hostname that ends in a top-level domain that is also a common
file extension or identifier in code (`.rs`, `.py`, `.md`, `.zip`, `.id`, `.name`, `.sh`
and similar) is not detected, so that `main.rs` and `user.name` stay untouched. The same
name inside a URL is still replaced.

## Checks

```
scripts/check.sh
```

This runs rustfmt, clippy with warnings as errors, and all tests.

## More

- `PLAN.md`: how the MVP is planned.
- `tickets/README.md`: shared rules, architecture and the ticket index.
