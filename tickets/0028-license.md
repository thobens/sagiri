---
id: "0028"
title: License
status: done
depends_on: []
---

# 0028 — License

## Why

The repository has no license, which means that nobody but the owner may use, copy or
package the code. A package (ticket 0029) has to name a license. The owner chose the dual
license that is usual for Rust projects: MIT or Apache-2.0, at the user's choice.

## Scope

- `LICENSE-MIT` in the repository root: the MIT license text with the copyright line
  `Copyright (c) 2026 <owner>`.
- `LICENSE-APACHE` in the repository root: the unmodified text of the Apache License,
  Version 2.0.
- `Cargo.toml` (workspace): `license = "MIT OR Apache-2.0"` under `[workspace.package]`.
- `crates/sagiri/Cargo.toml` and `crates/sagiri-core/Cargo.toml`: `license.workspace =
  true`.
- `README.md`: a short "License" section at the end, saying that Sagiri is licensed under
  either of the two, with the two file names.

### The owner's name

- `<owner>` is the author name of the repository's commits:
  `git log -1 --format=%an master`.
- The report states the name that was used, so that the owner can correct it before the
  merge.

## Out of scope

- License headers in source files.
- A contribution clause, a `NOTICE` file, a `COPYRIGHT` file.
- Checking the licenses of dependencies.
- `repository`, `homepage` or other Cargo metadata.
- Changing `publish = false`.

## Design notes

- **The Apache text must be exact.** Take it from
  `https://www.apache.org/licenses/LICENSE-2.0.txt`, or from the `LICENSE-APACHE` file of a
  crate in the local cargo registry (for example `anyhow`, under
  `~/.cargo/registry/src/*/anyhow-*/`), after checking that it is the standard text. Do not
  type it from memory and do not fill in the appendix's placeholder.
- **The MIT text is the standard one** from `https://opensource.org/license/mit`, with
  only the copyright line filled in.
- **SPDX expression.** `MIT OR Apache-2.0` is the form Cargo and Arch Linux both expect.
- Both files end with a single line break and have no trailing spaces, so that
  `cargo fmt` and editors leave them alone.

## Acceptance criteria

- [ ] `LICENSE-MIT` and `LICENSE-APACHE` exist in the repository root.
- [ ] `head -n 3 LICENSE-MIT` shows the copyright line with the year 2026 and the name
      printed by `git log -1 --format=%an master`.
- [ ] `grep -c "Permission is hereby granted, free of charge" LICENSE-MIT` prints 1.
- [ ] `diff LICENSE-APACHE <reference>` is empty, where the reference is the downloaded
      text or the registry file. The report names the reference and shows its
      `sha256sum`.
- [ ] `grep -c "Apache License" LICENSE-APACHE` is at least 1, and
      `grep -c "Version 2.0, January 2004" LICENSE-APACHE` prints 1.
- [ ] `cargo metadata --no-deps --format-version 1` shows `"license":"MIT OR Apache-2.0"`
      for both packages.
- [ ] `README.md` has the "License" section.
- [ ] `git diff master --stat` lists only the two license files, the three `Cargo.toml`
      files, `README.md` and this ticket.
- [ ] `scripts/check.sh` passes.

## Test plan

- The commands of the acceptance criteria.
