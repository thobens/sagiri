---
id: "0029"
title: PKGBUILD
status: todo
depends_on: ["0027", "0028"]
---

# 0029 — PKGBUILD

## Why

On Arch Linux and its derivatives, software that is installed with `make install` is
invisible to the package manager: it is not upgraded, not listed and not removed cleanly.
A PKGBUILD turns the checkout into a package that `pacman` installs and removes like any
other. This ticket adds one that builds from the local checkout.

## Scope

### `packaging/arch/PKGBUILD`

- `pkgname=sagiri`, `pkgrel=1`, `pkgver` equal to the version in the workspace
  `Cargo.toml` (today `0.1.0`).
- `pkgdesc`: the `description` of `crates/sagiri/Cargo.toml`, shortened to one sentence
  without the package name.
- `arch=('x86_64' 'aarch64')`, `license=('MIT OR Apache-2.0')`.
- `url`: the repository's web address, taken from `git remote get-url origin` and rewritten
  to its `https://` form. Without a remote, stop and report.
- `depends=('gcc-libs' 'glibc')`, `makedepends=('cargo' 'make')`,
  `optdepends=('libnotify: desktop notifications for session mode')`.
- `source=()`: nothing is downloaded. The checkout is found relative to the PKGBUILD:
  `_repo="$(realpath "$startdir/../..")"`.
- `prepare()`:
  - fails with a clear message if `pkgver` differs from the version in
    `$_repo/Cargo.toml`;
  - `cargo fetch --locked` for the host target, in `$_repo`.
- `build()`: `make -C "$_repo" release CARGO_FLAGS=--frozen`.
- `check()`: `cargo test --frozen --workspace`, in `$_repo`.
- `package()`:
  - `make -C "$_repo" install DESTDIR="$pkgdir" PREFIX=/usr`;
  - installs `LICENSE-MIT` and `LICENSE-APACHE` to
    `$pkgdir/usr/share/licenses/$pkgname/`, mode 644.

### Makefile

- New target `pkg`: runs `makepkg -f` in `packaging/arch`. It is listed by `make help`.

### `.gitignore`

- The directories and files that `makepkg` leaves in `packaging/arch`: `pkg/`, `src/` and
  `*.pkg.tar.*`.

### Documentation

`README.md`, new section "Installing":

- From source: `make release`, then `make install` with `PREFIX`.
- Arch Linux: `make pkg`, then `sudo pacman -U packaging/arch/sagiri-*.pkg.tar.zst`;
  removal with `sudo pacman -R sagiri`.
- Starting session mode at login with i3, repeating the two lines from the "Session mode"
  section.

## Out of scope

- Publishing to the AUR, a `-git` package that builds from the remote, tagged releases
  (backlog).
- A systemd user unit (backlog).
- Man pages, shell completions, a desktop file.
- Packages for other distributions.
- Installing the built package: that is the owner's step.

## Design notes

- **Local checkout as the source.** The repository has no release tags, and a package that
  builds from the remote could not be verified from an unmerged branch. Building what is
  on disk is what a personal install needs.
- **A fixed `pkgver` instead of a `pkgver()` function.** `makepkg` rewrites the PKGBUILD in
  place when `pkgver()` returns a new value, which would leave the checkout modified after
  every build. The comparison in `prepare()` keeps the fixed value honest.
- **`check()` runs the tests only.** That is the Arch convention for packages, and it
  guards the package against a broken build environment. It is not a second quality gate:
  format and lint checks stay in `scripts/check.sh` alone (ticket 0013).
- **The build runs in the checkout**, through the Makefile, so the install layout is
  defined once. `makepkg` may set `CARGO_TARGET_DIR`; the Makefile honours it (ticket
  0027).
- **No `libxcb` dependency.** The clipboard and X11 crates are pure Rust and speak the X
  protocol themselves.
- **Verify against the current Arch documentation** (the "Rust package guidelines" and
  `man PKGBUILD`): the exact `cargo fetch` invocation for the host target, whether
  `startdir` is available in every function used here, whether `options` such as `!lto`
  are needed for the build to succeed, and the SPDX form of `license`. Where the
  documentation differs from this ticket, follow the documentation and say so in the
  report.
- `namcap` is the Arch linter for PKGBUILDs and packages. It is optional.

## Acceptance criteria

Each is a command; the report shows the command and its result.

- [ ] `bash -n packaging/arch/PKGBUILD` succeeds.
- [ ] `(cd packaging/arch && makepkg --printsrcinfo)` succeeds and shows
      `pkgname = sagiri` and the license.
- [ ] `make pkg` succeeds and produces one `sagiri-*.pkg.tar.zst` in `packaging/arch`.
- [ ] `pacman -Qlp <package>` lists exactly these files (besides directories):
      `/usr/bin/sagiri`, `/usr/bin/sagiri-cli`, `/usr/bin/sagiri-session`,
      `/usr/share/licenses/sagiri/LICENSE-MIT`,
      `/usr/share/licenses/sagiri/LICENSE-APACHE`.
- [ ] `pacman -Qip <package>` shows the version `0.1.0-1` and the license.
- [ ] With `pkgver` temporarily changed to `9.9.9`, `make pkg` fails in `prepare()` with
      the version message. The change is reverted afterwards.
- [ ] After `make pkg`, `git status --porcelain` is empty: the build leaves no untracked
      or modified file that is not ignored.
- [ ] If `namcap` is installed: `namcap packaging/arch/PKGBUILD` and `namcap <package>`
      report no error. The report states whether `namcap` was run.
- [ ] `README.md` has the "Installing" section; `make help` lists `pkg`.
- [ ] `scripts/check.sh` passes.

Manual check, the owner's step (listed in the report as not done by the implementer):

- [ ] `sudo pacman -U <package>` installs; `sagiri --version`, `sagiri-cli --version` and
      `sagiri-session status` run from `/usr/bin`; `sudo pacman -R sagiri` removes
      everything.

## Test plan

- The commands of the acceptance criteria, on a machine with `makepkg` (Arch Linux or a
  derivative). On another system the implementer stops and reports that the ticket cannot
  be verified there.
