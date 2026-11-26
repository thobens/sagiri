---
id: "0035"
title: Release workflow with immutable releases
status: done
depends_on: ["0028", "0033", "0034"]
---

# 0035 — Release workflow with immutable releases

## Why

Sagiri can only be installed by building it from a checkout. A release gives users a
binary for their platform, and it gives the project a fixed point to refer to. For a tool
that people run over sensitive text, a release must also be trustworthy: what was published
under a version must never change afterwards, and a user must be able to check that an
archive was built by this repository's workflow from the tagged commit.

## Scope

### `.github/workflows/release.yml`

Triggers:

- `push` of a tag that matches `v[0-9]+.[0-9]+.[0-9]+`;
- `workflow_dispatch`, as a dry run: everything except the job `publish` and the
  attestation step.

`concurrency`: one run per tag, runs in progress are not cancelled. Top-level
`permissions: contents: read`; a job that needs more declares it itself.

Job `verify` (`ubuntu-latest`):

- checks out with the full history;
- on a tag: fails unless the tag is `v` followed by the workspace version, and unless the
  tagged commit is an ancestor of `origin/master`;
- runs `scripts/check.sh`.

Job `build` (needs `verify`), matrix with `fail-fast: true`:

| Target | Runner |
|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |

- Every target is built natively on a runner of its own architecture. No cross
  compilation.
- Steps: check out; install the toolchain as `ci.yml` does; install `cargo-audit` and
  `cargo-auditable` as `audit.yml` does, at the same versions;
  `make release CARGO='cargo auditable'`; `scripts/audit.sh` with the three built binaries;
  `scripts/package.sh <target>`; on a tag, a build provenance attestation for the archive
  with the official attestation action; upload the archive as a workflow artifact.
- Additional permissions of this job: `id-token: write` and `attestations: write`.

Job `publish` (needs `build`, only on a tag, `ubuntu-latest`), the only job with
`contents: write`:

1. downloads the three archives;
2. writes `SHA256SUMS` over them;
3. `gh release create <tag> --draft --verify-tag --generate-notes`, title
   `Sagiri <version>`;
4. `gh release upload` for the archives and `SHA256SUMS`;
5. `gh release edit <tag> --draft=false`.

If a draft release for the tag exists from an earlier failed run, it is deleted and
created again. If a published release for the tag exists, the job fails and changes
nothing.

### `scripts/package.sh`

`scripts/package.sh <target>` builds nothing. It turns the release binaries that are
already there into `dist/sagiri-<version>-<target>.tar.gz` and prints that path.

- It stages with `make install DESTDIR=<temporary directory> PREFIX=/usr/local` and packs
  the tree under `usr/local`, so the Makefile alone defines what is installed where.
- Archive layout, one top-level directory:

  ```
  sagiri-<version>-<target>/
    bin/sagiri
    bin/sagiri-cli
    bin/sagiri-session
    lib/systemd/user/sagiri-session.service    Linux targets only
    LICENSE-MIT
    LICENSE-APACHE
    README.md
  ```

- `<version>` is the workspace version, read from `cargo metadata`.
- File order in the archive is sorted, and owner and group are `root` with numeric id 0,
  so that the listing does not depend on the machine.
- Exit status 2 with a message if `<target>` is missing or a binary is not built.
- `.gitignore` gets `/dist/`.

### Pinned actions in all workflows

- Every `uses:` in `ci.yml`, `audit.yml` and `release.yml` names a full 40-character commit
  SHA, followed by a comment with the version tag it belongs to, for example
  `uses: actions/checkout@<sha> # v7.0.0`.
- Only actions of the `actions` organisation are used: checkout, upload-artifact,
  download-artifact and the attestation action.
- Every checkout step sets `persist-credentials: false`.

### Documentation

`README.md`:

- "Installing", new first subsection "From a release": download the archive for the
  platform and `SHA256SUMS`; check with `sha256sum -c --ignore-missing SHA256SUMS`; check
  the origin with `gh release verify <tag>` and
  `gh attestation verify <archive> --repo thobens/sagiri`; copy `bin/*` to a directory on
  the `PATH`; on Linux copy the unit as described in "Starting with systemd" (its
  `ExecStart` points to `/usr/local/bin`).
- The Linux archives are built on Ubuntu 24.04 and need a glibc at least that new. The
  macOS archive is for Apple silicon. On macOS `sagiri-session` only says that session
  mode is not supported there.
- New section "Releasing": set the version in the workspace `Cargo.toml` and `pkgver` in
  `packaging/arch/PKGBUILD` (and `pkgrel=1`), run `cargo build` once so that `Cargo.lock`
  follows, merge to `master`, create the tag `v<version>` on that commit and
  push the tag. A published release cannot be changed or repeated: a mistake needs a new
  version.

## Out of scope

- Creating the first tag and release. That is the owner's step.
- Repository settings (see below). The implementer does not change them.
- Publishing to crates.io (`publish = false` stays), an AUR package, packages for other
  distributions, Homebrew.
- Static (musl) builds, Windows, `x86_64-apple-darwin`.
- Man pages, shell completions, a changelog file, tools that bump versions.
- Signing with a key of the project. The attestation is the signature.

## Design notes

- **Two things make a release immutable.**
  - GitHub's repository setting "Immutable releases": once a release is published, its
    tag cannot be moved or deleted and its assets cannot be changed, added or removed.
    GitHub also records a release attestation that `gh release verify` checks.
  - Actions pinned by commit SHA: a tag such as `v7` can be moved to other code, a commit
    cannot. The build that produces the archives must not change under a fixed workflow
    file.
- **Draft, upload, publish.** With immutable releases a published release accepts no
  further asset, so everything is uploaded while the release is a draft. Publishing is the
  last step and the only one that cannot be repeated.
- **Everything that can fail runs before `publish`**: the quality gate, the builds, the
  audit of the exact binaries that are packed, and the packaging. A red run leaves at most
  a draft behind.
- **The tag must match the workspace version** so that `sagiri --version` and the
  release agree. The check also catches a tag pushed on the wrong commit.
- **The tag must be on `master`** so that a release never contains code that was not
  merged.
- **Native runners instead of cross compilation.** No extra linker or sysroot, and
  `cargo audit bin` and the tests run on the architecture they are for. Check that the
  arm runner label is still current and available for this repository.
- **`cargo auditable` builds.** The published binaries carry their dependency list, so a
  user can run `cargo audit bin` on an installed binary years later.
- **Build provenance** links each archive to the workflow run and commit that built it.
  It needs no key to manage. Artifact attestations and the arm runners depend on the
  repository's visibility and plan; if one of them is not available for this repository,
  stop and report.
- **`gh` instead of a release action.** The GitHub CLI is installed on the runners and is
  maintained by GitHub. It keeps the list of actions short.
- **One archive layout for all targets**, taken from `make install`. The session binary is
  part of the macOS archive too, where it does nothing useful: one list of binaries
  everywhere is simpler than an exception in the Makefile.
- **The dry run** exists because a release workflow that is only ever tested by releasing
  is tested too late. It produces the same archives as workflow artifacts.
- **Verify against the current GitHub documentation**: how immutable releases treat
  drafts, whether `--verify-tag` and `--generate-notes` work as described, the current
  name and inputs of the attestation action, the runner labels, and the newest version of
  every action. Where the documentation differs from this ticket, follow the documentation
  and say so in the report.

### Repository settings (the owner's steps)

Listed in the report as not done by the implementer:

- Settings → General → Releases: enable immutable releases.
- A tag ruleset for `v*` that restricts creation to the owner and blocks updates and
  deletions.
- Push `master`, then create and push the first tag.

## Acceptance criteria

Each is a command; the report shows the command and its result.

- [ ] `bash -n scripts/package.sh` succeeds; the file is executable.
- [ ] After `make release`, `scripts/package.sh x86_64-unknown-linux-gnu` creates
      `dist/sagiri-0.1.0-x86_64-unknown-linux-gnu.tar.gz` and `tar -tzf` lists exactly the
      files of the layout above, sorted.
- [ ] The unit in the archive contains `ExecStart=/usr/local/bin/sagiri-session run`.
- [ ] `scripts/package.sh` without an argument exits 2; with the binaries removed it exits
      2 and names the missing binary.
- [ ] After packaging, `git status --porcelain` is empty.
- [ ] The tag check, run locally as the workflow runs it, accepts `v0.1.0` and rejects
      `v9.9.9` and `0.1.0` with the version message.
- [ ] `grep -rnE 'uses:' .github/workflows` shows only lines with a 40-character hexadecimal
      reference and a version comment, and only actions of the `actions` organisation.
- [ ] Only the job `publish` has `contents: write`; `grep -n 'write' release.yml` shows
      that and the two attestation permissions of `build`.
- [ ] All three workflow files parse as YAML. If `actionlint` is available it reports no
      problem (the report states whether it was run).
- [ ] `README.md` has "From a release" and "Releasing".
- [ ] `scripts/check.sh` passes.

Only verifiable on GitHub; the report states for each whether it was verified:

- [ ] A `workflow_dispatch` run is green and offers three archives as artifacts.
- [ ] After the first tag: the release has three archives and `SHA256SUMS`,
      `sha256sum -c` succeeds, `gh release verify` and `gh attestation verify` succeed,
      the release is marked immutable and `gh release upload` to it is refused.

## Test plan

- The local commands of the acceptance criteria on Linux.
- The tag check is a script or a shell function that can be called with a tag name, so
  that it is tested without pushing a tag.
- The owner runs the dry run and the first release.
