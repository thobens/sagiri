---
name: sagiri-security-review
description: Reviews Sagiri code for security. Checks that no pasted text, config term or key material can leak (stderr, journal, panics, errors, files, process arguments, sockets, clipboard, terminal) and that the obfuscated output lets nobody draw conclusions about the originals (key derivation, what each rule keeps, detection misses, failures that deliver the original). Works on a ticket branch, a diff, one area or the whole repository, and ends with findings ranked by severity and a status for each security guarantee. Use this whenever the user asks for a security review, security audit, leak check, privacy check or threat-model check of Sagiri, asks whether a change, a rule or a dependency is safe, whether something can leak, be reversed or be guessed, or wants a ticket branch checked for security before it is merged, even if they do not name this skill. In this repository it replaces the generic security-review skill.
argument-hint: "[full | <ticket id or branch> | <path or area>]"
---

# Security review of Sagiri

Requested scope: `$ARGUMENTS`

Sagiri exists so that sensitive text can be shown to someone who should not see the
originals. Two things would defeat it, and this review looks for both:

1. **A leak.** Original text, a config term or the key gets out of the process some other
   way: a log line, a panic message, a file, a process argument.
2. **An inference.** The obfuscated text itself tells a reader more about the originals
   than it is meant to: a value that was not replaced, a replacement that can be reversed
   or narrowed down.

A generic checklist (injection, authentication, secrets in code) finds little here. The
review is driven by the guarantees in `references/guarantees.md`: what Sagiri promises,
against whom, and what it knowingly gives away. Every finding names the guarantee it
breaks and the adversary who profits.

## Rules for the review itself

- **Change nothing.** No edits, no commits, no branch switch, no files added to the
  repository. Build output is the only thing a review may cause. Fixes happen in tickets,
  after the review, because a reviewer who patches what they find stops looking for the
  next thing.
- **Never touch real user data.** Do not read `~/.local/share/sagiri/key` or
  `~/.config/sagiri/config.toml`, and do not print the values of `SAGIRI_KEY_FILE` or
  `SAGIRI_CONFIG`. Do not run a Sagiri binary without `--key-file` and `--config` pointing
  to temporary files. Do not use `--in`/`--out` with `clipboard` or `primary`, and do not
  start `sagiri-session run`, except through `scripts/x11-test.sh`, which uses a virtual
  display. A review that leaks the user's key into a transcript has done the damage it was
  meant to prevent. `scripts/probe.sh` of this skill takes care of all of this.
- **Fictional data in every probe and in the report**: documentation address ranges,
  addresses in the private blocks, made-up names, published test numbers.
- **A finding needs evidence**: a reproduction, or the complete path through the code from
  the source of the data to the place it gets out, with `file:line` for each step.
  Anything less is a hypothesis and is reported as one.
- **Say what was not checked.** A guarantee nobody looked at is "not verified", never
  "holds". In a review of one part, a guarantee that the part keeps is "holds within
  scope": the rest of the code was not looked at.

## 1. Settle the scope

| Argument | Scope |
|---|---|
| `full` | The whole repository at `HEAD` |
| A ticket id or a branch (`0041`, `ticket/0041`) | The change `master...<branch>`, read in the worktree where the branch is checked out (`git worktree list --porcelain`) |
| A path or an area (`crates/sagiri-core/src/rules/url.rs`, `session`, `rules`, `build`) | That part at `HEAD` |
| Nothing | The change `master...HEAD` if the current branch is ahead of `master`, otherwise `full`. Say which one it is |

Run `git status --porcelain` in the reviewed checkout. For a change it must print
nothing. For a path or `full`, name uncommitted files that lie in the scope in the report:
the review then describes a state that no commit holds.

Neither a diff nor a path is reviewed in isolation. The scope is the named code plus what
is needed to judge it: the callers of its functions, the functions it calls, the errors it
can return and where they are printed, the rules that share a primitive with it. Read
those as far as the question at hand needs, and no further. A review of one rule file
reads `pseudo.rs` for the primitives that rule uses; it does not review `pseudo.rs`.

## 2. Read the baseline

1. `references/guarantees.md`: the guarantees G1 to G8, the adversaries, the leakage
   budget and the accepted limits. Read it completely, every time.
2. `CLAUDE.md` (hard rules) and, in `tickets/README.md`, "Product decisions", "How
   consistency works" and "Threat model in one paragraph". For session mode and file
   handling also "Files Sagiri touches".
3. The tickets that built the code in the scope. The layout in `tickets/README.md` names
   the ticket for every file. A ticket says which behaviour is intended and why, for
   example which ordinary text a rule's rejections protect.
4. In `README.md`, what users are told about the scope: "Usage", "Configuration", and
   "Session mode" where it applies.

Two kinds of document, two roles. The tickets say what was intended. The user-facing
`README.md` says what users know. A limit counts as documented only if the `README.md`
states it: a user decides what to paste by the README and never sees a ticket.

If the code, these documents and `guarantees.md` disagree with each other, that is a
finding.

## 3. Run the passes

Choose the passes by what the scope contains. A full review runs all three.

| The scope contains | Pass | Guarantees |
|---|---|---|
| `sagiri-core/src/pseudo.rs`, `key.rs` | 2: "The primitives". 1: "Key file", "Memory" | G2, G3 |
| One or more files in `sagiri-core/src/rules/` | 2: "One rule at a time", "The output as a whole", "Cost", and "The primitives" for the ones the rule uses. 1: "Panics" | G4, G5, G6 |
| `engine.rs`, `rule.rs`, `config.rs` | 2: "The engine", "Allowlist and terms". 1: "Panics", "Debug and Display", "Errors from dependencies" | G4, G5, G6 |
| `crates/sagiri/src/` outside `session/`: binaries, `cli.rs`, `filter.rs`, `holder.rs`, `tui/` | 1: all sections | G1, G6, G7 |
| `crates/sagiri/src/session/`, `clipboard.rs`, `clipboard/` | 1: all sections, above all "Control socket" and "Selections and X11". 2: "Cost" | G1, G6, G7 |
| `Cargo.toml`, `Cargo.lock`, `.github/`, `scripts/`, `packaging/`, `Makefile` | 3 | G8 |

**Pass 1, where data can leave**: `references/channels.md`.

**Pass 2, what the output gives away**: `references/inference.md`.

Both files have a table of contents. Read the sections the table above names for the
scope, and the whole file for a full review.

**Pass 3, what is built**:

- Dependencies. For every crate that is new or changed in `Cargo.lock`: why it is needed,
  which features are on, and whether it brings a network client, a build script that
  fetches something, or a feature that re-enables `unsafe` (`x11rb`'s
  `allow-unsafe-code`). `cargo tree -p sagiri -e normal --locked` and
  `cargo tree -d --locked` show the tree and duplicate versions.
- `unsafe_code = "forbid"` is still in the workspace lints and no crate opts out.
- `scripts/audit.sh` checks the built binaries against RustSec. It needs the network,
  `cargo-audit` and `cargo-auditable`. If it cannot run, the audit is "not verified".
- Workflows, against "Workflows and automation" in `tickets/README.md`: actions pinned by
  commit, `permissions: contents: read` unless a job needs more, no secret besides the
  `GITHUB_TOKEN`, logic in `scripts/`. Also: no `pull_request_target`, and no
  `${{ github.event.* }}` value expanded inside a `run:` line.
- Automatic updates. A merged update must not change replacements (the golden vectors
  catch that) and must not change detection unnoticed. `psl` carries the public suffix
  list and `regex` the Unicode tables: both decide what is detected. Find the test that
  fails when detection changes; if there is none, that is a finding.
- The release: the tag check, `cargo auditable`, the audit, the attestation, and that
  nothing can replace a published archive.
- The systemd unit: where the service's stderr goes (the journal), and which hardening
  directives a user service that needs the X display can carry.

Two helpers, both read-only. `--root <worktree>` points them at another checkout:

```bash
S=.claude/skills/sagiri-security-review/scripts
$S/sinks.sh [--path rules/ip.rs] [category...]   # where data can get out or a process can stop
$S/probe.sh [file]                               # run fictional text through the filter
$S/probe.sh --corpus --kept                      # realistic paste formats that come out unchanged
```

`--help` on either lists the options. `scripts/corpus.txt` holds realistic formats per
kind: tool output, log lines, connection strings, headers. Run it in every review that
touches a rule or the engine, and compare with a run on `master` when reviewing a change:
a line that was `changed` there and is `kept` now is a regression.

For a full review the three passes are independent and each reads a lot of code. If the
Agent tool is available, run one `general-purpose` subagent per pass in parallel. Give
each the scope, the checkout path, the path of this skill directory, the rules above, and
the instruction to return candidate findings with `file:line` and evidence. Their reports
are claims: verify each one yourself in step 4 before it goes into the report.

## 4. Verify every candidate

For each candidate finding, in this order:

1. **Reproduce it**, or trace it completely. A probe line with its output, a failing
   throwaway test in a scratch directory, a `strace` line, or the chain of `file:line`
   from source to sink.
2. **Try to refute it.** Look for the check that prevents it, the test that pins the
   opposite, the caller that never passes such a value. Most candidates die here, and
   that is the point of the step.
3. **Name the adversary** from `guarantees.md`. If only an out-of-scope adversary profits,
   it is a hardening note.
4. **Compare with the leakage budget and the accepted limits.** Something the budget lists
   is not a finding. Something accepted but missing from the user-facing README is a
   documentation finding.
5. **Rate it.**

| Severity | Meaning |
|---|---|
| critical | An in-scope adversary gets original content or key material, or can reverse replacements, in normal use |
| high | The same, but it needs an error path, crafted input or an unusual configuration. Or: a realistic class of values is not replaced and the README does not say so. Or: a failure delivers the original and the user is not told |
| medium | The output reveals more than the budget allows without giving the value away. Or: an accepted limit that the README does not state. Or: a class of values that is not replaced in a format that is possible but uncommon |
| low | Hardening and defence in depth. A guarantee that holds today but that no test pins |

Rating a value that is not replaced:

- **Rate by what gets out, not by whether the rejection was intended.** Many rejections
  are deliberate and pinned by a test, because the same pattern occurs in ordinary text
  (a version number, a file name). The reader of the output gets the original either way.
  Intent decides the direction of the fix: change the rule where ordinary text can be
  told apart, state the limit in the README where it cannot. Say in the finding which
  ordinary text the rejection protects.
- **Realistic** means: the value appears in this form in the output of a common tool, in
  a common file format or in ordinary writing. Name the tool or the format. A form that
  needs an unusual setup is "possible but uncommon". A form that no person would read as
  the value (base64, split across lines) is outside what Sagiri promises.
- **Once the README states a limit, it is an accepted limit and no longer a finding.**
  Report related misses as one finding per cause, not one per probe line.

Then, for each guarantee in the scope, find the tests that would fail if it were broken.
The status of a guarantee is `holds` only with evidence: a test, or a completed trace.

## 5. Report

Report in the conversation, in this form. Do not write the report to a file unless asked.

```
# Security review: <scope> at <short commit>

<Two or three sentences: the result, and the most serious finding if there is one.>

## Guarantees
| Guarantee | Status | Evidence |
|---|---|---|
| G1 Content stays where the user put it | holds / violated / not verified / not in scope | tests, traces, finding numbers |
| ... one row per guarantee, G1 to G8 |

## Findings
### 1. <What is wrong, in one line> (<severity>)
- Guarantee and adversary: G<n>, A<n>
- Where: <file:line>
- What happens: <the mechanism, in two or three sentences>
- Evidence: <the probe input and output, the command, or the code path>
- Impact: <what the adversary learns, concretely; for a missing test or a gap in the
  documentation, what can go wrong unnoticed>
- Direction of a fix: <one or two sentences, and whether it changes a golden vector>

## Rules reviewed
<Only when the scope holds rules. Per rule, one line each: what `detect` rejects on
purpose, what `replace` keeps, where the original can pass through, and the smallest
space behind a replacement.>

## Decisions needed
<Behaviour that is confirmed but that the baseline does not settle: is it intended?
Each with what the answer would change.>

## Hypotheses
<Candidates that were neither confirmed nor refuted, each with what would settle it.>

## Accepted limits in this scope
<Each limit the scope relies on, and whether the README still states it.>

## What was run, and what was not verified
<Commands with their results. Passes or guarantees that were skipped, and why.>

SECURITY: pass
```

The report ends with the `SECURITY` line: `SECURITY: pass` if there is no finding of
severity critical or high, `SECURITY: findings` otherwise. Order the findings by severity.
Status values are `holds`, `holds within scope`, `violated`, `not verified` and `not in
scope`. Leave out a heading that has nothing under it, except "Guarantees" and "What was
run".

## 6. Afterwards

When a user is there to answer, offer after the report to turn each confirmed finding
into a ticket, and write tickets only if the user agrees. A review that runs as a
subagent or without a user ends with the report. A ticket follows `tickets/README.md` and the form of an existing bug ticket such
as `tickets/0038-overlong-term-panics.md`: the next free id, a row in the index, the
finding's evidence under "Why", and an acceptance criterion that pins the guarantee with a
test. One ticket per finding. The fix itself runs through `/ticket`, not in this review.

`references/guarantees.md` changes with the project's decisions. When the user answers a
"decision needed", accepts a limit, or a ticket changes what a rule keeps, propose the
edit to that file: a new row in the budget or in the accepted limits, or an entry removed
from "Open points". The baseline is only useful while it matches what was decided.
