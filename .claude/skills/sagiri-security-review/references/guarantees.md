# Security guarantees of Sagiri

This file is the baseline of a security review. It says what Sagiri promises, against whom,
and what it knowingly gives away. A review compares the code with this file.

- The code decides what happens. This file decides what is acceptable.
- A difference between the two is a finding in either direction: code that gives away more
  than this file allows, or this file promising something the code does not do.
- A limit that is accepted here but that the user-facing `README.md` does not state is a
  documentation finding. Users can only work around limits they know.

The tables were derived from the code at commit `43cb29b` (version 0.1.2). Verify every row
that the reviewed scope touches; do not trust the table over the code.

## Contents

- [What has to be protected](#what-has-to-be-protected)
- [Adversaries](#adversaries)
- [Guarantees](#guarantees)
- [Leakage budget](#leakage-budget)
- [Accepted limits](#accepted-limits)
- [Open points](#open-points)
- [Constraints on fixes](#constraints-on-fixes)

## What has to be protected

| Asset | Where it lives |
|---|---|
| Pasted text and every original value in it | process memory; the source the user chose (stdin, a selection); in the TUI, the screen |
| The secret key | the key file (mode 0600), process memory |
| `terms` and `allow` entries | the config file, process memory. Terms are sensitive by definition: they are the names the user wants hidden |
| The mapping original → replacement | nowhere. It is recomputed from the key every time and never stored |

## Adversaries

Name the adversary in every finding. A finding without one is usually not a finding.

| Id | Who | What they have |
|---|---|---|
| A1 | Reader of obfuscated output: the recipient of a paste, a chat service, a public issue tracker | Any amount of obfuscated text made with the same key, over a long time |
| A2 | A1 who also knows some originals | Pairs of original and replacement, for example a public hostname they recognise from context |
| A3 | A1 who can choose text | Can get text of their choice obfuscated under the victim's key and see the result: a document the victim obfuscates and sends back, a web page the victim copies from during a session |
| A4 | Author of the input | Writes the text that Sagiri processes: a log line from a system they influence, a crafted web page. Wants Sagiri to crash, hang, skip a value, or emit terminal control sequences |
| A5 | Another user on the same machine (different uid) | File system, `/proc/<pid>/cmdline`, world-readable directories, sockets in shared places |
| A6 | Reader of diagnostics | stderr output, the systemd journal, a bug report with a panic message, a CI log, a core dump, shell history |
| A7 | Supply chain | A dependency, a GitHub action, a release archive, an automatic update |
| A8 | Key thief | A copy of the key file |

Out of scope, and documented as such in `README.md` and `tickets/README.md`:

- Software running as the same user, and root. It can read the key file, the memory and
  the clipboard directly.
- Other clients of the same X display. X11 gives every client access to all selections.
- Physical access.

A finding that needs an out-of-scope adversary is at most a hardening note.

## Guarantees

### G1. Content stays where the user put it

Pasted text, original values, and `terms` and `allow` entries leave a Sagiri process only

- as obfuscated text on the destination the user chose (stdout, a selection),
- on the screen of the TUI, in the terminal the user started it in,
- through the pipe from `sagiri-cli` to its holder process, which carries the obfuscated
  text only.

They never appear in: stderr, the journal, a desktop notification, a tray tooltip, a
process argument, an environment variable, a file, the control socket, an error message, a
panic message, `Debug` output, or a network connection. Error messages name files, keys
and positions, never content.

Adversaries: A5, A6.

### G2. The key stays in the key file and in memory

- The key file is created with mode 0600, its missing parent directories with 0700, from
  the operating system's random source. A key file that group or others can access is
  refused.
- Key bytes are never printed, logged, passed as an argument or environment variable,
  written anywhere else, or reachable through a public API or `Debug`.
- Copies of the key in memory are zeroized when dropped.
- The key is used only as an HMAC key.

Adversaries: A5, A6, A8 (stealing the key must not be easier than reading a 0600 file).

### G3. No replacement can be inverted without the key

Every part of a replacement that is not kept on purpose (see the leakage budget) is a
function of `HMAC-SHA256(key, "sagiri/v1" 0 domain 0 input)` and of nothing else. Without
the key, a replacement is as good as random within its shape.

- Each use of the key has its own domain label, and one label is used with one primitive
  (`stream`, `permute`, `mimic`) and one encoding of its input.
- Bijections (`permute`, `permute_within`) stay bijections on exactly the intended set.
- Sampling is unbiased (`below` uses rejection sampling).
- No randomness comes from anywhere but `Pseudonymizer`. The `rand` crate is not used in
  `sagiri-core`.

Adversaries: A1, A2.

### G4. The output reveals only what the leakage budget lists

About an original value, the obfuscated text reveals its kind, the parts kept on purpose,
its shape where the budget says so, and whether it equals another value. The
[leakage budget](#leakage-budget) lists all of it per kind. Anything beyond that list is a
finding.

Adversaries: A1, A2, A3.

### G5. A value of a supported kind is replaced

A value of a kind that the README says Sagiri replaces is found and replaced wherever it
appears in a realistic text, unless the README names the exception. Realistic means: in
the output of a common tool, in a common file format, or in ordinary writing. A miss puts an
original into the output: it is the most direct leak there is, and the recipient cannot
tell it from a replacement, but the user believes it is gone.

This guarantee cannot be absolute. Its testable form: there is no class of misses that is
both realistic and missing from the README. Whether a rejection was deliberate does not
matter for the guarantee; a deliberate one that the README does not state still breaks it.

Adversaries: A1 (benefits from every miss), A4 (provokes misses).

### G6. Failures do not deliver the original

- An error produces no output on stdout and does not set a selection.
- In session mode, a text that cannot be replaced is not left on the selection: the
  selection is cleared and the user is told. If clearing fails too, the user is told that
  the original is still there.
- A panic, a hang or a crash on crafted input must not turn into "the original was
  delivered and nobody noticed". In session mode an unreplaced clipboard is exactly that.
- No code path silently returns the original for a value that was detected. Each
  intentional pass-through is listed in the leakage budget.

Adversaries: A4.

### G7. Local only

No Sagiri process opens a network connection or resolves a name. The only child processes
are `sagiri-cli --hold <selection>` (text through a pipe) and `notify-send` with fixed
arguments. The control socket carries one of five command words, and the answer is
`active`, `inactive` or `error` with a fixed reason.

Adversaries: A5, A7.

### G8. What is built is what was reviewed

- No `unsafe` in the workspace (`unsafe_code = "forbid"`), and no dependency feature that
  re-enables it (`x11rb`'s `allow-unsafe-code`).
- Release binaries are built with `cargo auditable`, audited against RustSec, and
  published with a build provenance attestation from pinned actions.
- An automatic dependency update cannot change a replacement: the golden vectors fail
  first.
- An automatic dependency update that changes what is *detected* is caught by a test, or
  it is merged by a person.

Adversary: A7.

## Leakage budget

What the output gives away by design. "Shape" means: the number of characters, which
positions hold letters, digits or other characters, the case of each letter, and every
character that is neither a letter nor a digit (it is copied as it is). Shape is kept
where a row says so. IP addresses do not keep it: the output is written in canonical form.

| Kind | Kept in the clear | Replaced | Input of the derivation |
|---|---|---|---|
| IPv4 | The block: `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, `100.64/10`, or public. Not replaced at all: `0/8`, `127/8`, and `224.0.0.0` and above | The rest of the address, by a bijection inside the block. The spelling is not kept | In a private block: the host bits, under a label per block. Public: the whole address. Leading zeros do not matter |
| IPv6 | `::`, `::1` and `ff00::/8` are not replaced. Link-local keeps the first 64 bits, unique-local the first byte, global unicast (`2000::/3`) the first 3 bits, everything else the first 16 bits. An IPv4-mapped address follows the IPv4 row | The remaining bits, from a stream | All 16 bytes. The output is written in canonical form, so the original spelling is lost |
| Hostname | The public or internal suffix (`com`, `co.uk`, `corp`, `internal`, `lan`, `local`, `home.arpa`, ...); the number of labels; the shape of every label. Reserved names are not replaced: `example.com`, `example.net`, `example.org` and everything under them, and names ending in `example`, `invalid`, `localhost`, `test` | Every label left of the suffix | Per label: the lower-cased label and everything to its right. So two names share the replacement of a common parent (`a.corp.acme.com`, `b.corp.acme.com`), and the output shows which names share one |
| URL | Scheme, port, path, query keys, fragment, percent-escapes, all punctuation. Path and fragment change only where another rule finds a value in them. A query part without `=` or with an empty value is kept. A host made of digits and dots that is not an IPv4 address is kept | Host (as hostname or IP), userinfo, query values | Host: as above. Userinfo and query values: the value and its position |
| Email | The shape of the local part; the domain's kept parts as for a hostname. An allowed domain is kept, the local part is still replaced | Local part and domain | Local part: the whole lower-cased address. Domain: as a hostname, so the same domain gets the same replacement in an address, a URL and on its own |
| Payment card | The brand prefix (1 to 4 digits), the number of digits, the grouping | All other digits; the check digit is recomputed | The digits |
| IBAN | The country code, the length, the grouping, digit or letter at each position, the case | Everything after the country code; the check digits are recomputed | The IBAN in upper case without spaces |
| Phone | `+`, the country code, the number of digits, the grouping, a trunk group `(0)` | All digits after the country code | `+` and the digits |
| Term | The shape, including spaces and punctuation inside the term | Letters and digits | The lower-cased term |

For every kind the output also reveals:

- **Equality.** The same original gives the same replacement, always. A reader sees which
  values repeat, how often, and which documents share a value. This is the product, not a
  defect.
- **Count and position.** How many values a text had and where.
- **Everything Sagiri does not replace.** The words around a value often identify it.
  Sagiri replaces values, not context.

Consequences of the budget that a review treats as accepted, as long as the README says
them in plain words:

- **Guessing with chosen text (A3).** Whoever can get chosen text obfuscated under the
  victim's key can test guesses: obfuscate the candidates, compare. For small spaces (an
  IPv4 block, a short hostname label, a list of known customer names) this recovers the
  original. Consistency makes this unavoidable. It is a finding only if something makes it
  cheaper than one chosen value per guess, or if the documentation does not warn about it.
- **Frequency (A1).** The most frequent replacement is the most frequent original.
- **Known pairs (A2).** A known pair reveals that one pair, plus what the hostname
  hierarchy shares. It must reveal nothing about other values.

## Accepted limits

| Limit | Where it is documented |
|---|---|
| With the key, small value spaces such as IPv4 can be enumerated | `tickets/README.md` (Threat model) and the crate documentation of `sagiri-core`. `README.md` only says to keep the key private |
| Session mode: the original is on the clipboard between a copy and its replacement, and a clipboard manager may record it | `README.md` (Session mode) |
| Session mode is no boundary against other software on the same machine | `README.md` (Session mode) |
| Session mode: files, images and file names are not changed; text selected but not copied is pasted unchanged by a middle click unless `primary = "watch"` | `README.md` (Session mode) |
| A bare hostname under a top-level domain that is also a common file extension or identifier is not detected | `README.md` (Known limitation) |
| The config file holds the sensitive terms in plain text | `README.md` (Configuration) |
| In the TUI the original is visible on screen | by design: it is the review screen |
| Pasted text in memory is not zeroized, and may reach swap or a core dump | not documented; hardening only (out-of-scope adversary) |
| Kinds on the backlog are not replaced: MAC addresses, secrets and tokens, national phone formats, national identifiers, person names, UUIDs | `tickets/README.md` (Backlog); check what `README.md` says |

Before relying on a row, open the document it names and check that the sentence is still
there and still true.

## Open points

Known and not decided. A review that meets one of these cites it under "Decisions needed"
instead of reporting it as new, and does not rate it. An entry leaves this list when the
project decides: it becomes a row of the budget, an accepted limit, or a ticket.

- **A value inside a value of another kind follows the rule that wins the overlap.** An
  address inside a hostname (`192.0.2.10.nip.io`, `ip-192-0-2-10.eu.compute.internal`,
  `10.2.0.192.in-addr.arpa`) is replaced as hostname labels: the number of digits of each
  part survives, and the result is not linked to the replacement of the bare address. The
  hostname row allows the shape of a label; the IPv4 row allows only the block. Which row
  governs is not decided.
- **The README does not state the leakage budget.** It says that values get a
  realistic-looking fake value, and names one limit for hostnames. What each kind keeps,
  and that a consistent mapping can be tested with chosen text, is written only here and
  in the tickets. Until the README says it, the "accepted" consequences of the budget are
  accepted by the project but unknown to users.

## Constraints on fixes

A review proposes directions for fixes. These rules limit them:

- **Golden vectors are a compatibility contract.** A fix that changes any replacement for
  an existing key is a breaking change. It needs its own ticket and a new scheme label
  next to `sagiri/v1`. Say so in the finding; do not propose it as a small patch.
- **A fix that makes detection stricter can mangle ordinary text.** Rules validate before
  they report, because a false positive damages the user's text. Weigh a miss against the
  false positives its fix would cause, and say which one the finding accepts.
- **No `unsafe`.** Hardening that needs `libc` calls (`prctl`, `mlock`, `setrlimit`) is
  not available in this workspace. Look for a safe route (a systemd directive, a
  documented recommendation) or record the limit.
