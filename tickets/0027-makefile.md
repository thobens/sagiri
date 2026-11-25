---
id: "0027"
title: Makefile
status: done
depends_on: ["0016", "0020"]
---

# 0027 — Makefile

## Why

The package now builds three binaries. Building one of them in release mode, or installing
all of them, takes cargo commands that nobody remembers. A Makefile gives these tasks short
names and gives packaging (ticket 0029) one place that knows what is installed where.

## Scope

A `Makefile` in the repository root, for GNU Make.

### Variables

| Variable | Default | Meaning |
|---|---|---|
| `CARGO` | `cargo` | the cargo command |
| `CARGO_FLAGS` | `--locked` | passed to every build |
| `PREFIX` | `/usr/local` | installation prefix |
| `DESTDIR` | empty | staging root, prepended to `PREFIX` |
| `BINS` | `sagiri sagiri-cli sagiri-session` | not meant to be overridden |

The directory of the built binaries is `$(CARGO_TARGET_DIR)/release` if `CARGO_TARGET_DIR`
is set in the environment, otherwise `target/release`.

### Targets

| Target | Does |
|---|---|
| `all` (default) | the same as `release` |
| `build` | debug build of the workspace |
| `release` | release build of all three binaries |
| `sagiri`, `sagiri-cli`, `sagiri-session` | release build of that one binary (`cargo build --release -p sagiri --bin <name>`) |
| `install` | copies the three release binaries to `$(DESTDIR)$(PREFIX)/bin` with mode 755 |
| `uninstall` | removes them from there |
| `check` | runs `scripts/check.sh` and nothing else |
| `clean` | `cargo clean` |
| `help` | prints the targets and variables |

- `install` does not build. If a binary is missing it stops with
  `run 'make release' first` and installs nothing.
- All targets are declared `.PHONY`.

### Documentation

- `README.md`, "Build and run": `make`, `make sagiri-cli`, `make install PREFIX=~/.local`,
  `sudo make install`, `make check`.
- `CLAUDE.md`, "Commands": one line for `make release` and `make install`. The quality gate
  line stays `scripts/check.sh`.

## Out of scope

- A `pkg` target and the PKGBUILD (ticket 0029).
- Man pages, shell completions, desktop files.
- Stripping, cross-compiling, release profiles in `Cargo.toml`.
- Any second place that runs format, lint or test commands.

## Design notes

- **`install` never runs cargo.** `sudo make install` would otherwise build as root and
  leave root-owned files in `target/`.
- **`check` only delegates.** `scripts/check.sh` is the quality gate (ticket 0013); the
  Makefile must not grow its own list of checks.
- **`DESTDIR` and `PREFIX` follow the usual convention**, so that the PKGBUILD can call
  `make install DESTDIR="$pkgdir" PREFIX=/usr`.
- **Recipes use `install -Dm755`**, which creates the directory.
- The Makefile contains no pasted text, key or other content concern; it is build tooling
  only.

## Acceptance criteria

Each is a command; the report shows the command and its result.

- [ ] `make -n all` succeeds and its output contains `--locked`.
- [ ] `make release` succeeds; `target/release/sagiri`, `target/release/sagiri-cli` and
      `target/release/sagiri-session` exist and are executable.
- [ ] After `cargo clean`, `make sagiri-cli` produces `target/release/sagiri-cli` and its
      output shows `--bin sagiri-cli`.
- [ ] `make install DESTDIR="$tmp" PREFIX=/usr` (with `tmp` from `mktemp -d`) creates
      exactly three files under `$tmp/usr/bin`, each with mode 755.
- [ ] `make uninstall DESTDIR="$tmp" PREFIX=/usr` removes them.
- [ ] After `cargo clean`, `make install DESTDIR="$tmp"` fails with the hint and creates no
      file under `$tmp`.
- [ ] `CARGO_TARGET_DIR="$tmp/t" make release install DESTDIR="$tmp/d"` installs from
      `$tmp/t/release`.
- [ ] `grep -nE 'cargo +(fmt|clippy|test)|\$\(CARGO\) +(fmt|clippy|test)' Makefile` prints
      nothing.
- [ ] `make -n check` prints `scripts/check.sh` and no cargo command.
- [ ] `make help` lists every target of the table.
- [ ] `README.md` and `CLAUDE.md` are updated as described.
- [ ] `scripts/check.sh` passes.

## Test plan

- Run the commands of the acceptance criteria in order, with a temporary directory for
  every installation. Nothing is installed outside it.
