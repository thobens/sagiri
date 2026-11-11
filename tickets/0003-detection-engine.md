---
id: "0003"
title: Detection and replacement engine
status: todo
depends_on: ["0002"]
---

# 0003 — Detection and replacement engine

## Why

Every kind of sensitive value is found and replaced by its own rule. The engine is the one
place that runs the rules over a text, decides which match wins when two overlap, and
produces the output. It also has to give the TUI enough information to highlight findings
and to switch single findings off.

## Scope

In `crates/sagiri-core`: `rule.rs`, `engine.rs`, `rules/mod.rs`. Implement the API pinned in
`tickets/README.md` ("Core API").

### `rule.rs`

- `Kind` with all nine variants, in the pinned order. Derive `Copy`, `Eq`, `Hash`, `Ord`,
  `Debug`.
  - `Kind::ALL: [Kind; 9]`
  - `id(self) -> &'static str`: `term`, `url`, `email`, `ipv4`, `ipv6`, `hostname`,
    `payment_card`, `iban`, `phone`
  - `label(self) -> &'static str`: `Term`, `URL`, `Email`, `IPv4`, `IPv6`, `Hostname`,
    `Card`, `IBAN`, `Phone`
- `Rule` trait as pinned.
- `Context` with `pseudo()`, `nested()` and `is_allowed()`.
  - `nested(fragment)` runs `Engine::obfuscate` on the fragment, one nesting level deeper.
    At depth 8 it returns the fragment unchanged.
  - `is_allowed` returns `false`. Ticket 0012 wires it up.

### `engine.rs`

- `Engine::new`, `Engine::context`.
- `Engine::analyze(text) -> Vec<Finding>`, in this order:
  1. Run `detect` of every rule, in rule order.
  2. Discard ranges that are empty, out of bounds, not on `char` boundaries, or that contain
     `\n` or `\r`.
  3. Resolve overlaps: sort candidates by start ascending, then length descending, then
     rule position ascending. Walk the list and accept a candidate only if it does not
     overlap one already accepted. Adjacent ranges do not overlap.
  4. Call `replace` for each accepted candidate.
  5. Drop a finding whose replacement equals the original, or whose replacement contains
     `\n` or `\r`.
  The result is sorted by `span.start`.
- `render(text, findings, include)`: builds the output. A finding for which `include`
  returns `false` keeps its original text. `Rendered::spans` has one entry per finding, in
  order, with its range in the output text and the `replaced` flag. Text between findings
  is copied byte for byte.
- `Engine::obfuscate(text)`: `analyze`, then `render` with everything included, returning
  the text.

### `rules/mod.rs`

- `default_rules()` returning an empty `Vec`. Later tickets add to it.

## Out of scope

- Any real rule (tickets 0004 and later).
- The allowlist and the config file (ticket 0012).
- Reading input, key loading, the binary (ticket 0005).

## Design notes

- **Two phases.** `analyze` is the expensive part and runs once per paste. `render` is
  cheap and runs again whenever the user toggles a finding in the TUI.
- **Overlap rule.** "Earliest, then longest" makes a URL win over the hostname inside it
  and an email win over its domain. The outer rule then delegates the inner part through
  `Context::nested` or a direct function call, which keeps the mapping consistent.
- **No-op findings are dropped** so that a rule can decline a value (for example
  `localhost`) simply by returning it unchanged.
- **Line breaks.** The TUI scrolls the original and the obfuscated text in lockstep. That
  only works if both have the same number of lines, hence the checks in steps 2 and 5.
- `nested` must only be given a strict fragment of the matched string. The depth limit is a
  safety net against a rule that passes its whole match.

## Acceptance criteria

All tests use small fake rules defined in the test module (for example "every `foo`" or
"every run of digits").

- [ ] With no rules, or with rules that match nothing, `obfuscate` returns the input
      unchanged, including for the empty string.
- [ ] Overlap cases, each a named test: identical ranges from two rules (earlier rule
      wins); one range inside another (outer wins); partial overlap (earlier start wins);
      adjacent ranges (both kept).
- [ ] Invalid ranges from a rule (empty, out of bounds, inside a multi-byte character,
      containing `\n`) are ignored without a panic.
- [ ] A rule that returns the original is not reported as a finding.
- [ ] A rule whose replacement contains `\n` is not applied.
- [ ] For every `RenderedSpan` with `replaced == true`,
      `rendered.text[span.range] == findings[span.finding].replacement`; with
      `replaced == false` it equals `original`.
- [ ] `render` with `include = |_| false` returns the input text unchanged.
- [ ] Property test: for arbitrary Unicode text and a fake rule, the output with all
      findings removed equals the input with all matched spans removed, and the number of
      lines is unchanged.
- [ ] `Context::nested` test: a fake outer rule that calls `nested` on part of its match
      gets the inner rule's replacement for that part.
- [ ] A fake rule that calls `nested` on its entire match terminates (depth limit).
- [ ] `Kind::ALL` has nine distinct entries; `id()` values are unique.
- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests in `engine.rs` and `rule.rs`; one `proptest` property as described.
- No golden vectors: this ticket contains no derivation of its own.
