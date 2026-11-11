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

## Checks

```
scripts/check.sh
```

This runs rustfmt, clippy with warnings as errors, and all tests.

## More

- `PLAN.md`: how the MVP is planned.
- `tickets/README.md`: shared rules, architecture and the ticket index.
