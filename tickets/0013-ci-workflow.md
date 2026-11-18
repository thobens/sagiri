---
id: "0013"
title: CI workflow
status: done
depends_on: ["0001"]
---

# 0013 — CI workflow

## Why

`scripts/check.sh` is the quality gate, but it only helps if it runs on every change. A CI
workflow runs it on each push and pull request, on both supported platforms, so a broken
branch is caught before review.

## Scope

- **Precondition.** The repository has no remote yet. This ticket assumes it will be hosted
  on GitHub. Confirm that with the owner before starting; if the answer is no, stop and
  report.
- `.github/workflows/ci.yml`:
  - Triggers: `push` to `master`, and `pull_request`.
  - `permissions: contents: read`.
  - `concurrency`: one run per branch or pull request, cancelling runs in progress.
  - One job `check` with a matrix over `ubuntu-latest` and `macos-latest`,
    `fail-fast: false`.
  - Steps: check out the repository; `rustup show` (this installs the toolchain and
    components named in `rust-toolchain.toml`); `scripts/check.sh`.
- `README.md`: one line saying that CI runs `scripts/check.sh` on Linux and macOS.

## Out of scope

- Caching, release builds, publishing, packaging.
- Code coverage, dependency audits, scheduled runs.
- Windows runners (Windows is not a target for the MVP).
- Branch protection settings.

## Design notes

- **The workflow only calls `scripts/check.sh`.** CI and a developer's machine must run
  exactly the same checks; nothing is duplicated in YAML.
- **No third-party actions.** `rustup` is preinstalled on GitHub-hosted runners and reads
  `rust-toolchain.toml`, so only the official checkout action is needed. Use its current
  major version.
- **No cache for now.** The project is small; a cache adds a third-party action and
  failure modes for little gain. Revisit when build times hurt.
- **Tests need no display and no clipboard.** They use a fake clipboard and ratatui's
  `TestBackend`. If a test fails only in CI for that reason, the test is wrong, not the
  workflow.

## Acceptance criteria

- [ ] `.github/workflows/ci.yml` exists and parses as YAML.
- [ ] If `actionlint` is available, it reports no problems (state in the report whether it
      was run).
- [ ] The workflow contains no build, lint or test command other than `scripts/check.sh`.
- [ ] The only action used is the official checkout action.
- [ ] `scripts/check.sh` passes locally.
- [ ] After the first push to GitHub, the `check` job is green on both platforms. This can
      only be verified once a remote exists; the report states whether it was.

## Test plan

- Parse the workflow file as YAML locally.
- Run `scripts/check.sh` locally.
- Verify the first CI run after the repository is pushed.
