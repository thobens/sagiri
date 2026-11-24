---
id: "0030"
title: Reject a `rules` value that is not a table
status: done
depends_on: ["0018"]
---

# 0030 — Reject a `rules` value that is not a table

## Why

The top-level `rules` key of the config file is deserialized into the derived serde struct
`RuleToggles`. A derived struct also accepts a sequence and fills its fields in declaration
order, so a TOML array of booleans is taken as a list of toggles. `rules = [false]` parses
and switches the term rule off without any message; elements after the ninth are ignored.

Ticket 0012 made unknown keys an error so that a typo cannot silently disable protection the
user thinks is in place. An array in place of the `[rules]` table is the same kind of
mistake and has to fail the same way.

Behaviour on `master` (commit `3433de9`, which includes ticket 0018), observed with a
throwaway test through `Config::load`:

| Config file | Result today |
|---|---|
| `rules = [false]` | accepted, `term` is off |
| `rules = [false, false]` | accepted, `term` and `url` are off |
| `rules = []` | accepted, all defaults |
| nine booleans, the last `false` | accepted, `phone` is off |
| ten booleans | accepted, the tenth is ignored |
| `rules = [false, "hush"]` | error `` `rules`: expected a table `` at line 1, column 17 |
| `rules = [[false]]` | error `` `rules`: expected a table `` at line 1, column 10 |
| `[[rules]]` followed by `term = false` | error `` `rules`: expected a table `` at line 1, column 1 |
| `rules = "x"`, `rules = true`, `rules = 1` | error `` `rules`: expected a table `` at line 1, column 9 |
| `[rules]` with `term = false`, `rules = { term = false }`, `rules.term = false` | accepted, `term` is off |
| `[rules]` alone, `rules = {}` | accepted, all defaults |

The first five rows are the bug. The next two rows already fail, but at the position of the
element that is not a boolean instead of the position of the value.

Ticket 0018 found the same flaw for `session` and closed it with a check in `parse`: after
`DeTable::parse`, the value of `session` must be a `DeValue::Table`, otherwise the error
`` `session`: expected a table `` is returned at the start of the value. On the same commit
`session = [false]` is rejected at line 1, column 11. `rules` was left out there as out of
scope.

## Scope

### `sagiri-core/src/config.rs`

- The check that ticket 0018 added to `parse` for `session` covers `rules` as well: one
  check for both keys, not a second copy. For each of the two keys that is present, a value
  that is not a `DeValue::Table` ends parsing with a `ConfigError::Toml` before
  `toml::from_str` runs.
- **Error text** for `rules`, in the format the file already uses:

  ```
  <path>: line <L>, column <C>: `rules`: expected a table
  ```

  - `<L>` and `<C>` are the position of the first character of the value: column 9 for
    `rules = [false]` on its own line.
  - For `[[rules]]` the position is on the line of the first `[[rules]]` header. The column
    is whatever the value's span gives.
  - The detail is this fixed text. It contains no value from the file.
- `locate()`: the arm that reports `rules` as "expected a table" can no longer be reached.
  It is removed and replaced by a comment, as ticket 0018 did for `session`.
- The comment at the check names both keys.

Nothing else changes: `Config`, `RuleToggles`, `ConfigError`, the public API pinned in
`tickets/README.md`, and every message for errors inside a table (`rules.phone`,
`session.primary`, unknown keys).

## Out of scope

- `allow` and `terms`. A table or a scalar there is already rejected with
  `expected an array of strings`.
- Any change to how `session` behaves. Its message, its position and its tests stay as
  ticket 0018 left them.
- A hand-written `Deserialize` implementation or a `deserialize_with` helper for
  `RuleToggles` or `SessionConfig`.
- New keys, new rule toggles, changes to the user-facing `README.md`: the documented forms
  of `[rules]` keep working unchanged.

## Design notes

- **Why a check before deserializing.** The derived `Deserialize` of a struct accepts both a
  map and a sequence, and `toml` hands an array to it as a sequence. `deny_unknown_fields`
  does not help, because a sequence has no field names. The check on the parsed `DeTable`
  is the mechanism ticket 0018 chose; using it for both keys keeps one place that decides
  what may be a table.
- **Fail closed.** An array that happens to hold only booleans is not a shorter way to
  write toggles. It is rejected even when every element is `true` and even when it is empty.
- **Inline tables and dotted keys are tables.** `rules = { term = false }` and
  `rules.term = false` are parsed to `DeValue::Table` and must keep working, as must an
  empty `[rules]` header and `rules = {}`.
- **Both keys wrong at once.** Which of the two is reported first is not pinned and no test
  depends on it.
- **A later key whose value is a table** needs to be added to the same check. A comment at
  the check says so.

## Acceptance criteria

Unit tests in `config.rs`, in the style of `session_that_is_not_a_table_is_error`:

- [ ] `rules_array_is_error`: `rules = [false]`, `rules = []`, `rules = [false, false]`,
      an array of nine booleans, an array of ten booleans, `rules = [false, "hush"]` and
      `rules = [[false]]` each give an error whose message contains
      `` `rules`: expected a table `` and `config.toml: line 1, column 9`, and does not
      contain `hush`.
- [ ] `rules_scalar_is_error`: `rules = "hush"`, `rules = true` and `rules = 1` give the
      same message at the same position, without `hush`.
- [ ] `rules_array_of_tables_is_error`: `[[rules]]` followed by `term = false` gives the
      same message with `config.toml: line 1`.
- [ ] `rules_error_position_is_the_value`: with `rules = [false]` on the third line of a
      file, after an `allow` and a `terms` line, the message contains `line 3, column 9`.
- [ ] `rules_table_forms_parse`: `[rules]` with `term = false`, `rules = { term = false }`
      and `rules.term = false` each give `term == false` and every other toggle `true`; an
      empty `[rules]` header and `rules = {}` give `RuleToggles::default()`.

End-to-end test in `crates/sagiri/tests/config.rs`:

- [ ] `rules_array_exits_1`: with a config file holding `terms = ["Acme Corp"]` and
      `rules = [false]`, and input that contains the term, `sagiri` exits 1, stdout is
      empty, and stderr names `rules` and does not contain the term.

And:

- [ ] Every existing test passes unchanged, including
      `structural_errors_name_the_key_and_hide_values`, `all_nine_toggles_are_accepted`,
      `example_parses`, `session_that_is_not_a_table_is_error` and all golden vectors.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests next to the existing ones in `config.rs`, using the `load_str`
  helper.
- One end-to-end test with temporary key and config files, next to the existing
  unknown-key test.
