---
id: "0007"
title: Email address rule
status: todo
depends_on: ["0006"]
---

# 0007 — Email address rule

## Why

Email addresses are personal data and appear in almost every pasted log, ticket or mail
thread. The replacement must look like an address, and its domain must match what the same
domain becomes elsewhere in the text.

## Scope

`crates/sagiri-core/src/rules/email.rs` with `EmailRule`, registered in `default_rules()`
at its pinned position.

- **Detection.** `local@domain` where
  - the local part is 1–64 characters from `A-Z a-z 0-9 . _ % + -`, not starting or ending
    with a dot, without two dots in a row. Where the text violates this, match the longest
    valid tail: leading dots are left out, and so is everything up to the last `..`;
  - the domain has two or more labels (same label syntax as ticket 0006), a purely
    alphabetic last label of at least two letters, and
    `hostname::has_known_suffix(domain)` is true;
  - the character before the match is not alphanumeric or one of `_ % + -`;
  - the character after the match is not alphanumeric, `_` or `-`, and is not a `.`
    followed by an alphanumeric character.
- **Replacement.**
  - Domain: `hostname::pseudonymize(domain, ctx)`.
  - Local part: `ctx.pseudo().mimic("email-local", seed, local)` with `seed` = the whole
    address in lower case. Separators (`.`, `_`, `+`, `-`, `%`) stay where they are.
  - The `@` is kept.
- **Canonical form:** the whole address in lower case. `John.Doe@ACME.com` and
  `john.doe@acme.com` give the same letters, each in the case of its input.

## Out of scope

- Addresses with quoted local parts, comments, or IP-literal domains (`user@[10.0.0.1]`).
- Internationalized addresses.
- Treating `john` and `john+news` at the same domain as related: they are different
  addresses and get unrelated replacements.
- Keeping well-known mail providers (`gmail.com`) readable. That is a job for the allowlist
  in ticket 0012.

## Design notes

- The domain goes through the same function as bare hostnames and URL hosts, so one domain
  has one replacement everywhere.
- A reserved or allowlisted domain (for example `example.com`) is kept, but the local part
  is still replaced: the person is the sensitive part.
- The known-suffix check is what rejects `icon@2x.png`.
- `git@github.com:org/repo.git` is detected as the address `git@github.com`. That is
  intended: the host is hidden consistently.
- The email rule comes before the hostname rule and its match starts earlier, so the
  engine prefers it over the bare domain.

## Acceptance criteria

- [ ] `john.doe@acme.com` becomes `xxxx.xxx@B.com` where `B.com` is exactly what bare
      `acme.com` maps to, and the local part keeps its length and the dot.
- [ ] The same address twice in a text, and in two engines with the same key, gets the
      same replacement.
- [ ] `John.Doe@ACME.com` maps to the same letters as `john.doe@acme.com` with the input's
      capitalization.
- [ ] `first_last+tag-1@mail.acme.co.uk` keeps `_`, `+`, `-`, the digit position and
      `co.uk`.
- [ ] `jane@example.com` keeps `example.com` and changes `jane`.
- [ ] Must match (address only): `<john@acme.com>`, `mailto:john@acme.com`,
      `"john@acme.com"`, `Contact john@acme.com.`, `john@acme.com,`,
      `git@github.com:org/repo.git` (as `git@github.com`).
- [ ] Longest valid tail: `.john@acme.com` matches `john@acme.com`; `john..doe@acme.com`
      matches `doe@acme.com`.
- [ ] Must not match: `@mention`, `foo@bar`, `a@b.c`, `icon@2x.png`, `user@localhost`.
- [ ] In `https://acme.com/u?mail=john@acme.com` the query value is replaced exactly as the
      bare address (through `Context::nested` in the URL rule).
- [ ] In `https://john:pw@acme.com/x` the finding is one URL, not an email address.
- [ ] Golden vectors with the test key: five addresses, as literals.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests in `rules/email.rs`.
- `crates/sagiri-core/tests/golden_email.rs`: golden vectors and the two URL interaction
  cases through `Engine` with `default_rules()`.
