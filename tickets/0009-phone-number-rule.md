---
id: "0009"
title: Phone number rule
status: done
depends_on: ["0003"]
---

# 0009 — Phone number rule

## Why

Phone numbers are personal data and common in signatures, contact lists and support
tickets. Detecting them in every national format produces many false positives, so the MVP
handles the unambiguous international form and leaves the rest to the backlog.

## Scope

`crates/sagiri-core/src/rules/phone.rs` with `PhoneRule`, registered in `default_rules()`
at its pinned position.

- **Detection.** A `+`, then digits, optionally separated.
  - Separators between digit groups: a single space, hyphen or dot. A group may be wrapped
    in parentheses, as in `+1 (415) 555-0132`.
  - A trunk-prefix group `(0)` directly after the country code is allowed. It is kept
    verbatim, and its digit neither counts nor is replaced.
  - 8 to 15 digits in total, including the country code.
  - The number starts with a valid country calling code (1–3 digits). Embed the list of
    assigned ITU-T E.164 country codes as a constant.
  - The `+` is not preceded by an alphanumeric character or another `+`.
  - The match is not followed by a digit. Trailing separators are not part of the match.
- **Replacement.**
  - Keep `+` and the country code.
  - Draw every other digit from `stream("phone", canonical)`. The first digit after the
    country code is drawn from 2–9; the others from 0–9.
  - Put every separator, parenthesis and `(0)` group back at its original position.
- **Canonical form:** `+` followed by the digits, without separators and without a `(0)`
  group. `+41 79 123 45 67`, `+41791234567` and `+41 (0)79 123 45 67` get the same digits.

## Out of scope

- National formats without `+` (`079 123 45 67`, `(415) 555-0132`) and the `00` prefix.
  Backlog: national formats with a configurable default region.
- Validating number length or prefixes per country.
- Extensions (`ext. 123`) and vanity numbers.
- The libphonenumber metadata: do not add it for the MVP.

## Design notes

- Only the `+` form is detected because a run of digits with spaces could be anything: an
  order number, a date, coordinates. The leading `+` and a valid country code make the
  match reliable.
- The country code is kept because it carries meaning ("a Swiss number") without
  identifying a person.
- The first national digit is drawn from 2–9 so that the result does not start with a
  trunk prefix or an invalid area code in most numbering plans.

## Acceptance criteria

- [ ] Detected and replaced: `+41 79 123 45 67`, `+41791234567`, `+41 (0)79 123 45 67`,
      `+1 (415) 555-0132`, `+1-415-555-0132`, `+44 20 7946 0958`, `+49.30.901820`,
      `Tel: +41 79 123 45 67.`, `(+41 79 123 45 67)`.
- [ ] Every replacement keeps the `+`, the country code, the number of digits and every
      separator and parenthesis at its position.
- [ ] `+41 79 123 45 67`, `+41791234567` and `+41 (0)79 123 45 67` get the same digits; the
      `(0)` group is still present in the third.
- [ ] The first digit after the country code of every replacement is between 2 and 9.
- [ ] Not detected: `+1` (too short), `+999 12 345 67 89` (unassigned country code),
      `a+41791234567`, `2+41791234567`, `++41791234567`, `079 123 45 67`,
      `0041 79 123 45 67`, a `+` followed by 16 digits, `+41  79 123 45 67` (double space).
- [ ] A trailing sentence dot or closing parenthesis is not part of the match.
- [ ] The same number twice in a text, and in two engines with the same key, gets the same
      replacement.
- [ ] Golden vectors with the test key: four numbers, as literals.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests in `rules/phone.rs`.
- `crates/sagiri-core/tests/golden_phone.rs` for the golden vectors through `Engine` with
  `default_rules()`.
