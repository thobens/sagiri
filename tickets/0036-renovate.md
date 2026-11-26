---
id: "0036"
title: Renovate with automerge rules
status: todo
depends_on: ["0013", "0034", "0035"]
---

# 0036 — Renovate with automerge rules

## Why

Dependencies age. Updating them by hand happens rarely and then all at once, which is the
worst way to find out which update broke something. Renovate opens one pull request per
update. Small updates that pass CI are merged without anybody's time. Two limits keep that
safe: a waiting period, so that a release that is withdrawn or found to be malicious within
its first days never reaches the repository, and no automatic merge for updates that may
break the build or the behaviour.

## Scope

### `.github/renovate.json5`

Renovate runs as the hosted Mend Renovate GitHub App. This ticket adds its configuration
and nothing else.

The rules, for every dependency that Renovate manages here:

| Update | Pull request | Merge |
|---|---|---|
| major | at once | by a person |
| minor of a `0.x` version (`0.30` → `0.31`) | at once | by a person |
| minor of a version from `1.0` on | 7 days after the release | automatic, if all checks pass |
| patch of any version | 7 days after the release | automatic, if all checks pass |
| every other update type | at once | by a person |

Settings that implement them (option names as in the Renovate documentation of
2026-11):

- `extends`: `config:recommended`, `helpers:pinGitHubActionDigests` and
  `customManagers:githubActionsVersions`.
- Managers: `cargo` (the workspace `Cargo.toml` files and `Cargo.lock`), `github-actions`
  (the pinned actions of ticket 0035), and the custom manager of the preset for the
  `*_VERSION` variables in the workflows (ticket 0034).
- `minimumReleaseAge: "7 days"` together with `internalChecksFilter: "strict"` on the
  automerged rows: Renovate creates neither a branch nor a pull request before the release
  is seven days old.
- `automerge: true` on the automerged rows only. `platformAutomerge: false`.
- `packageRules` that tell `0.x` versions apart with `matchCurrentVersion`.
- `rangeStrategy: "bump"`.
- `lockFileMaintenance` disabled.
- The Dependency Dashboard issue stays on: it lists the updates that are waiting for their
  seven days.
- For `x11rb` and `signal-hook`: a note in the pull request body (`prBodyNotes`) that
  `cargo tree -p sagiri -i <crate>` must show one version after the update (rule from
  `tickets/README.md`).

The file is JSON5 so that each rule has a comment that says what it is for.

### Workflows

- The `*_VERSION` variables of `audit.yml` and `release.yml` get the comment line that the
  `customManagers:githubActionsVersions` preset expects (data source `crate`, the crate
  name).

### Documentation

- `README.md`, new section "Dependency updates": the table above in words, that the
  golden vector tests in CI are what keeps an automatic update from changing replacements,
  and that an update with a failing check stays open for a person.

## Out of scope

- Installing the app and the repository settings (see below).
- Running Renovate in a workflow of this repository.
- Pull requests from security advisories (`vulnerabilityAlerts`, `osvVulnerabilityAlerts`).
  Ticket 0034 reports advisories.
- Lock file maintenance for transitive dependencies (backlog).
- Grouping of updates, schedules, assignees, labels.
- Updating any dependency. The first pull requests come from Renovate.

## Design notes

- **A `0.x` minor update counts as major.** Cargo treats `0.30` → `0.31` as incompatible,
  and most dependencies here are `0.x` crates (`ratatui`, `crossterm`, `hmac`, `sha2`,
  `x11rb`, `toml`). Renovate labels such an update "minor". Without the extra rule the
  breaking updates of most of the dependency tree would be merged automatically.
- **Major updates get no waiting period.** The owner's rule gives the seven days to the
  updates that are merged without review. A person reads a major update before merging
  it, and seeing it early helps planning.
- **No branch before the seven days are over** (`internalChecksFilter: "strict"`), instead
  of a pull request that waits with a pending check. Nothing with the new version exists in
  the repository until then, and nobody merges it by hand too early.
- **`platformAutomerge: false`.** Renovate then merges a pull request itself, on a later
  run, and only when every check on it has passed. GitHub's own auto-merge depends on
  required checks in the branch rules; without them it can merge before CI has finished.
  The slower merge is the price for not depending on a repository setting.
- **"All checks" includes the audit** of ticket 0034, which runs on pull requests that
  change `Cargo.lock`. An update that brings in a vulnerable version is not merged.
- **The golden vectors are the safety net.** An update of `psl` changes the public suffix
  list and can change what the hostname rule detects; an update of a hashing crate must
  not change any output. Both would fail a golden vector test, the check fails, and the
  pull request stays open. Such a pull request must never be fixed by changing the vector.
- **`psl` publishes often**, because every list update is a patch release. If its pull
  requests become noise, a schedule for that one crate is a later change.
- **`rangeStrategy: "bump"`**: the version in `Cargo.toml` is raised together with
  `Cargo.lock`, so `Cargo.toml` always names a version that was tested.
- **No lock file maintenance.** It updates every transitive dependency at once and cannot
  honour the waiting period. Vulnerable transitive dependencies are found by ticket 0034.
- **Pinned action digests are kept by Renovate.** It updates the SHA and the version
  comment together, and the pinning preset pins an action that somebody adds by tag.
- **Verify against the current Renovate documentation**: every option and preset name
  above, the exact form of `matchCurrentVersion` for `0.x` versions with the `cargo`
  versioning and for action versions with a leading `v`, the comment format of the custom
  manager preset, and what Renovate does when a release has no timestamp. Where the
  documentation differs from this ticket, follow the documentation and say so in the
  report.

### Repository settings (the owner's steps)

Listed in the report as not done by the implementer:

- Install the Mend Renovate app for this repository only.
- Optional but recommended: a ruleset on `master` that requires the `check` jobs of
  `ci.yml`.
- `master` must be pushed: Renovate reads the configuration from the default branch on
  GitHub.

## Acceptance criteria

- [ ] `.github/renovate.json5` exists and is valid JSON5.
- [ ] `renovate-config-validator` (for example
      `npx --yes --package renovate -- renovate-config-validator .github/renovate.json5`)
      reports no error and no migration. The report shows the output, or states that the
      tool could not be run and why.
- [ ] The report has a table that names, for each of these updates, the matching rule and
      the result (when the pull request is opened, who merges):
      `clap` 4.6.7 → 4.6.8, `clap` 4.6 → 4.7, `regex` 1 → 2, `ratatui` 0.30.2 → 0.30.3,
      `ratatui` 0.30 → 0.31, `actions/checkout` to a new major version,
      `actions/checkout` to a new patch version, `cargo-audit` in `CARGO_AUDIT_VERSION`
      to a new patch version.
- [ ] If Renovate can be run locally in dry-run mode (`--platform=local`), its output
      lists the Cargo dependencies, the actions and the `*_VERSION` variables. The report
      shows the list or states that this was not run.
- [ ] Every `*_VERSION` variable in `.github/workflows` has the comment line of the
      preset.
- [ ] `README.md` has the "Dependency updates" section.
- [ ] `scripts/check.sh` passes.

Only verifiable after the app is installed; the report states whether it was verified:

- [ ] The Dependency Dashboard issue appears and lists the detected dependencies.
- [ ] No onboarding pull request is opened.
- [ ] A patch update that is older than seven days gets a pull request and is merged after
      a green CI run. A major update gets a pull request and stays open.

## Test plan

- The validator and, if possible, a local dry run.
- The rule table is checked by the reviewer against the configuration file, rule by rule.
- The live behaviour is observed by the owner over the first weeks.
