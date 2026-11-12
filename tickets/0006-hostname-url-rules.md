---
id: "0006"
title: Hostname and URL rules
status: done
depends_on: ["0003", "0004"]
---

# 0006 — Hostname and URL rules

## Why

Hostnames and URLs name the systems, companies and projects a text is about. They are also
where consistency matters most: `git.acme.com` must get the same replacement whether it
stands alone, inside a URL or, later, in an email address, and `www.acme.com` should visibly
belong to the same replaced domain.

## Scope

`crates/sagiri-core/src/rules/hostname.rs` (`HostnameRule`) and `rules/url.rs` (`UrlRule`),
both registered in `default_rules()` at their pinned positions. Add the `psl` crate.

### Hostname mapping

`pub fn pseudonymize(host: &str, ctx: &Context<'_>) -> String`

1. Return the host unchanged if `ctx.is_allowed(host)` or the host is reserved:
   - `localhost`
   - any name whose last label is `localhost`, `example`, `invalid` or `test`
   - `example.com`, `example.net`, `example.org` and their subdomains
2. Work on the lower-case form. A single trailing dot is set aside and re-appended.
3. Find the suffix: the public suffix according to `psl` (ICANN and private sections,
   including its default rule that treats an unknown last label as the suffix), or
   `home.arpa`. A single-label host has no suffix.
4. Keep the suffix. Replace every other label with
   `ctx.pseudo().mimic("hostname", seed, label)`, where `seed` is the lower-case name from
   that label to the end. For `git.acme.com` the label `acme` is seeded with `acme.com`
   and the label `git` with `git.acme.com`.
5. A label that starts with `xn--` is replaced by a `mimic` of the label without that
   prefix.

Because `mimic` takes the case from the original label, `ACME.com` becomes the upper-case
form of what `acme.com` becomes.

Also expose `pub fn has_known_suffix(host: &str) -> bool`: true if the suffix is a known
entry of the public suffix list or one of the internal suffixes `local`, `internal`, `lan`,
`corp`, `intranet`, `localdomain`, `home.arpa`. Ticket 0007 uses it.

### Bare hostname detection (`HostnameRule::detect`)

A label is 1–63 characters of ASCII letters, digits and hyphens, not starting or ending
with a hyphen. A **chain** is the longest run of two or more labels joined by single dots.
The character before a chain is not alphanumeric; the character after it is not
alphanumeric. A trailing sentence dot is not part of the chain.

A chain is **accepted** as a hostname if all of the following hold:

- Its last label is purely alphabetic and at least two letters long.
- Its total length is at most 253.
- `has_known_suffix` is true.
- Its last label is not in `AMBIGUOUS_TLDS`.
- It is not in `NOT_HOSTNAMES` (compared in lower case).
- It is not reserved (see step 1 above).
- It is not directly followed by `(`.

If the chain is not accepted and its last label is in `FILE_EXTENSIONS`, drop that label
and test the shorter chain, repeatedly. A chain shortened this way is accepted only with a
suffix from the public suffix list, not with an internal suffix. This finds the domain in
`acme.com.conf` and `acme.com.tar.gz` without turning `settings.local.json` into a host.

A `%XX` escape (two hex digits) directly before a chain is a boundary: the chain starts
after the escape and the escape stays byte-identical. This keeps hosts inside
percent-encoded URLs (`https%3A%2F%2Fgit.acme.com`) mapped like the bare host.

The three lists are heuristics. Keep each as one sorted constant so it is easy to extend.

`AMBIGUOUS_TLDS`, top-level domains that are also common file extensions or identifiers in
code:

```
ac am app audio auto bar box call cc cf city cloud company contact date dev email events
foo global group host id in info int java la link live map md media mk ml mov name network
new next now one online open page phone pl pm prod ps pub py read rs run search services
sh site so space store stream style support swift team tf tools top video work works
world zip zone
```

`NOT_HOSTNAMES`, well-known names from code that look like hostnames:

```
env.local java.io java.net javax.net kotlin.io scala.io std.io std.net system.io system.net
```

`FILE_EXTENSIONS`, labels that may trail a hostname inside a file name:

```
bak bz2 cer cfg conf crt csr css db gz htm html ini js json key lock log old orig p12 pem
pfx php pid pub service sock sql tar tgz tmp toml txt xml xz yaml yml zip zone
```

### URL detection and replacement (`UrlRule`)

- **Detection.** A scheme (`[A-Za-z][A-Za-z0-9+.-]*`), then `://`, then everything up to
  whitespace or one of `<>"` and backtick. Then trim from the end: any of `.,;:!?'`, and a
  closing `)`, `]` or `}` that has no matching opener inside the URL. The scheme must not be
  preceded by an alphanumeric character.
- **Replacement.** Split the text by hand into scheme, userinfo, host, port, path, query and
  fragment, and splice the result back together. Do not serialize through the `url` crate:
  everything that is not replaced must stay byte-identical.

  | Part | Treatment |
  |---|---|
  | scheme, `://`, port | kept |
  | userinfo | user and password separately, as for query values below but with domain `url-userinfo`: percent escapes are kept and each piece between escapes gets its own `mimic` |
  | host, IP literal (with or without brackets) | `ctx.nested(address)` |
  | host, anything else | `hostname::pseudonymize(host, ctx)`; this also maps single-label hosts |
  | path | `ctx.nested(path)` |
  | query | split on `&`, each pair at the first `=`. Keys are kept. A non-empty value becomes `ctx.nested(value)`; if that leaves it unchanged, `mimic("url-query", value, value)` |
  | fragment | `ctx.nested(fragment)` |

  A percent escape (`%` and two hex digits) is copied unchanged by the query-value `mimic`
  step: apply `mimic` to the pieces between escapes. Piece number `n` (from 0, one step per
  preceding escape, empty pieces included) is seeded with the whole value followed by `n`
  as 4 big-endian bytes, so same-shape pieces differ. A value without escapes is one piece
  with `n = 0`. The same applies to userinfo values.

  A host made only of digits and dots that is not a dotted-quad IPv4 address (`1.2.3`) is
  left unchanged.

  If nothing changes, return the original; the engine then drops the finding.

## Out of scope

- Obfuscating URL paths (backlog). Paths are kept; only values that another rule
  recognizes inside them are replaced.
- Internationalized hostnames in Unicode form.
- URLs without `://` (`mailto:`, `tel:`, `urn:`).
- Making the suffix lists or `AMBIGUOUS_TLDS` configurable.

## Design notes

- **Label-wise mapping** keeps the hierarchy visible: all hosts under one domain share that
  domain's replacement. Seeding each label with its full parent path means `www` under
  `acme.com` and `www` under another domain get different replacements.
- **The public suffix is kept** (`.com`, `.co.uk`, `github.io`). It makes the result look
  like the original and reveals little.
- **`AMBIGUOUS_TLDS` trades missed detections for fewer false positives.** Pasted code is
  full of `user.name`, `item.id`, `main.rs`. A bare name ending in one of these is not
  reported; the same name inside a URL or an email address still is. This is a known
  limitation and belongs in the README.
- **The tables in the acceptance criteria are authoritative.** If an entry cannot be
  satisfied without breaking another one, report it instead of dropping it.
- **Pinned current behaviour, no special handling:** `10.0.0.5.nip.io` is a hostname and maps
  label-wise (`97.9.9.8.dun.io` with the test key); wildcard public-suffix rules such as
  `*.ck` are kept as suffixes.
- **`nested` for the IP host** means the IP rule's categories and exclusions apply, and an
  address in a URL maps exactly like the same address elsewhere.
- The `regex` crate has no look-around: check neighbouring characters in code.

## Acceptance criteria

- [ ] Hierarchy: with `git.acme.com → A.B.com`, the mappings `www.acme.com → C.B.com` and
      `acme.com → B.com` share the same `B`; `A != C`.
- [ ] Shape: every replaced label has the length and character classes of the original
      (`db-prod-01.corp.internal` keeps its hyphens, digits and suffix).
- [ ] Case: `ACME.com` maps to the upper-case form of the `acme.com` mapping; a trailing dot
      is preserved.
- [ ] Suffix: `www.acme.co.uk` keeps `co.uk`; `acme.github.io` keeps `github.io`.
- [ ] Reserved names are untouched: `localhost`, `example.com`, `www.example.org`,
      `foo.test`.
- [ ] Bare detection, must match: `git.acme.com`, `acme.com`, `www.acme.co.uk`,
      `db01.corp.internal`, `printer.local`, `acme.io`, `(git.acme.com)`,
      `see git.acme.com.` (without the final dot), `git.acme.com:22` (without the port).
- [ ] Bare detection, must match `acme.com` only: `*.acme.com`,
      `/etc/letsencrypt/live/acme.com/fullchain.pem`, `sites-enabled/acme.com.conf`,
      `backup_acme.com.tar.gz`.
- [ ] `_dmarc.acme.com` matches `dmarc.acme.com` and leaves the underscore in place.
- [ ] Bare detection, must not match: `main.rs`, `README.md`, `script.py`, `Main.java`,
      `main.tf`, `archive.zip`, `Cargo.toml`, `index.html`, `jquery.min.js`,
      `settings.local.json`, `.env.local`, `user.name`, `item.id`, `foo.bar`, `e.g.`,
      `v1.2.3`, `1.2.3.4`, `java.lang.String`, `java.io.File`, `import java.io.*;`,
      `using System.Net;`, `com.acme.Service.run(x)`, `process.env.NODE_ENV`, `obj.com(x)`.
- [ ] URL host equals bare host: in `https://git.acme.com/team/repo` the host is replaced
      exactly as bare `git.acme.com`; scheme and path are unchanged.
- [ ] URL with IP host: in `http://10.0.0.5:8080/health` and `http://[2001:db8::1]:8080/x`
      the address is replaced exactly as the bare address; the port is unchanged.
- [ ] `postgres://admin:s3cret@db01.corp.internal:5432/app?sslmode=require`: user,
      password, host and the query value change; scheme, port, path and the key `sslmode`
      do not.
- [ ] Single-label host: `http://db-prod-01:5432/` gets a same-shape host.
- [ ] Nested values: in `http://acme.com/nodes/10.0.0.5/status?peer=10.0.0.6` both
      addresses are replaced exactly as the bare addresses.
- [ ] `http://localhost:3000/api` and `https://example.com/` produce no finding;
      `http://localhost:3000/api?token=abc123` changes only the token.
- [ ] Byte fidelity: `HTTPS://Git.Acme.COM:443/A%20b?x=1%2F2#Frag` keeps scheme case, port,
      path, both percent escapes and the fragment.
- [ ] Trailing punctuation: `See https://git.acme.com/x.`, `(https://git.acme.com/x)`,
      `<https://git.acme.com/x>` and `[t](https://git.acme.com/x)` match without the
      trailing character; `https://acme.com/wiki/Foo_(bar)` keeps its `)`.
- [ ] The same query value in two different URLs gets the same replacement.
- [ ] Golden vectors with the test key: five hostnames and three URLs, as literals.
- [ ] `README.md` documents the ambiguous-TLD limitation in one short paragraph.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests in `rules/hostname.rs` and `rules/url.rs`.
- `crates/sagiri-core/tests/golden_hostname_url.rs`: golden vectors and the
  cross-rule cases (URL with IP host, nested values) through `Engine` with
  `default_rules()`.
