# Pass 2: what the output gives away

Covers G3 (no inversion), G4 (leakage budget), G5 (detection) and the pass-through part of
G6 of `guarantees.md`.

The question of this pass: given only obfuscated text, and perhaps some known or chosen
originals, what can be concluded about an original value? Everything that can be concluded
must be in the leakage budget.

## Contents

- [The primitives](#the-primitives)
- [One rule at a time](#one-rule-at-a-time)
- [The engine](#the-engine)
- [Allowlist and terms](#allowlist-and-terms)
- [The output as a whole](#the-output-as-a-whole)
- [Cost](#cost)
- [Probing](#probing)

## The primitives

`crates/sagiri-core/src/pseudo.rs`, `key.rs`

Read these two files completely whenever the scope touches them, and whenever a rule in
the scope uses a primitive in a new way.

**Domain separation.** Every HMAC input is `"sagiri/v1" 0 domain 0 message`.

- The encoding must be injective: two different (domain, message) pairs never give the
  same HMAC input. The domain may not contain a NUL byte (asserted); the message comes
  last.
- The three primitives build different messages under the domain: `stream` appends the
  caller's input and a 4-byte counter, `permute` appends `"feistel"`, the round, the width
  and the right half, `mimic` is a `stream` whose input may get a 4-byte retry counter
  appended. A domain that is used with two primitives, or with two different encodings of
  its input, lets one use collide with the other. List every domain label in the scope
  with its primitive and its input encoding:

  ```bash
  grep -rnE '\.(stream|permute|permute_within|mimic)\(|mimic_keeping_escapes\(|within\("' \
      crates/sagiri-core/src/rules crates/sagiri-core/src/engine.rs | grep -vE '^[^:]+:[0-9]+: *//'
  ```

  Each label must appear with one primitive and one input encoding. A label built from
  input (`format!`) is a finding.
- Within one domain, different values must give different inputs. Look for encodings that
  concatenate variable-length parts without a separator or a length.

**The input of a derivation is the canonical form of the whole value**, or deliberately a
part of it (a hostname label and everything to its right). Two risks:

- Too little in the input: two different originals share an input, so they share a
  replacement where the shape allows it. That tells a reader they are related.
- Something in the output that is neither kept on purpose nor derived: see "One rule at a
  time".

**`permute` (Feistel).** Balanced, `ROUNDS` rounds, round function is truncated HMAC over
round, width and right half. Odd widths walk the cycle of the next even width.

- Check the bijection on the exact set, also for `permute_within`: the result is a member,
  and distinct members map to distinct members. The tests should try every value of a
  small width.
- Small domains are the weak spot of every format-preserving construction. The smallest
  ones here are 16 bits (`192.168/16`, `169.254/16`). An adversary with known pairs (A2)
  learns those pairs for certain; the question for the review is whether pairs tell
  anything about *other* values. If the number of rounds, the round function or a width
  changed, look up the current analysis of Feistel networks on small domains (the attacks
  on FF1 and FF3 and the minimum domain size NIST set in response) before judging, and do
  not judge from memory.
- A bijection has fixed points: about one value per domain maps to itself. The engine
  drops a finding whose replacement equals the original. A reader cannot tell a fixed
  point from any other replacement, so nothing is revealed; but the value is shown as "not
  found" in the TUI. Note it only if something makes fixed points more likely than chance.

**`stream`, `below`, `pick`.** Counter mode over HMAC. `below` rejects instead of taking a
plain modulo. A change that introduces `% n` on stream output without rejection is a bias
finding. Check that a stream is never used for more than one value, and that the order of
calls on a stream does not depend on anything but the shape.

**`mimic`.** Letters become letters, digits become digits, everything else is copied. So
the shape is in the output in full. Check what "everything else" can be for each caller:
for a term or a query value it may be most of the value.

- The retry (`out != shape`) guarantees that a replacement differs from its original. It
  also tells a reader that the original is not the replacement, which removes one
  candidate. That is negligible unless the space is tiny: think about one-letter and
  one-digit shapes.
- Collisions: `mimic` is not a bijection. Two originals of the same shape can get the same
  replacement. For a short label the output space is small (two letters: a few hundred).
  A collision makes two different originals look equal. That is a correctness problem
  for the reader and no leak; mention it only if a scope change makes it more frequent.

**The key.** 32 bytes from `getrandom`, used only as the HMAC key. Any other use (as a
seed for something else, as part of a message, compared in non-constant time against
input, truncated) is a finding.

## One rule at a time

`crates/sagiri-core/src/rules/`

For every rule in the scope, answer these in order. Write the answers down; they go into
the report as the evidence for G4 and G5.

1. **What does `detect` reject?** Every `continue`, every `return None`, every condition
   in an `&&` chain is a class of values that is not reported. For each class, build the
   shortest realistic text that falls into it and run it (see [Probing](#probing)). Then
   decide: documented in the README, a deliberate protection against false positives (say
   which ordinary text it protects), or a finding.

   Contexts that rules commonly get wrong. Try each for every kind in the scope:

   - the character before and after: `_`, `-`, `.`, `/`, `=`, `:`, `@`, quotes, brackets,
     a letter of another script, the start and the end of the text;
   - the value in an assignment, a path, a file name, a JSON string, a log prefix, a shell
     variable (`HOST_10.20.30.40`, `ip=...`, `"...",`, `<...>`);
   - the value at the end of a sentence, followed by a period or a comma, and in
     parentheses;
   - other spellings: upper case, mixed case, leading zeros, IPv6 compression and zone
     ids, an IPv4 address as a decimal or hexadecimal number, a trailing dot on a
     hostname, punycode, a card or phone number with other separators;
   - encodings: percent-encoding, JSON escapes (`.`, `\/`), HTML entities, a value
     broken over two lines, a zero-width character inside the value;
   - the value inside another value: a hostname in a URL path, a URL in a query value, an
     email address in a URL, an IP address in a hostname.

   Formats matter more than single characters. Most real misses come from the way a
   common tool prints a value: `address.port` in a packet capture, `address: message` in
   an error, `label:value` in a certificate dump, an escaped line break in a JSON log,
   prose in a script that writes no spaces. `probe.sh --corpus --kept` runs a collection
   of such formats (`scripts/corpus.txt`) and prints the lines that came out unchanged.
   Start there, then add the formats of the tools that produce the kind under review.

   Not every miss is a finding. A value written so that no human would read it as that
   value (base64, split over lines) is outside what a pattern-based tool promises; say so
   in one line and move on. How to rate the others is in step 4 of `SKILL.md`: by what
   gets out, not by whether the rejection was intended.

2. **What does `replace` put into the output?** Go through the replacement character by
   character. Each one is one of:

   - kept on purpose, and listed in the leakage budget;
   - derived through `Pseudonymizer`;
   - formatting of the original that is re-applied (case, separators, grouping), and
     covered by "shape" in the budget;
   - computed from derived parts only (a check digit).

   A character that is none of these is a finding. So is a kept part that the budget does
   not list.

3. **Where does `replace` return the original?** Search the rule for every return of the
   input: `original.to_string()`, `host.to_string()`, `_ => pair.to_string()`. For each,
   determine whether a value that `detect` reported can reach it. If it can, the engine
   drops the finding and the original stays (G6). Each such path must be listed in the
   budget or be unreachable, with the reason.

4. **What is the canonical form?** Two spellings of one value must give one replacement.
   Check the reverse too: can two *different* values have the same canonical form?

5. **How large is the space behind a replacement?** Given the shape and the kept parts,
   how many originals are possible: an IPv4 address in `192.168/16`, a two-letter host
   label, a phone number with a known country code and length. This number is what an
   adversary with chosen text (A3) or with the key (A8) has to try. It is accepted, but
   the report should state the smallest spaces of the scope, because users decide by it
   what they can paste where.

6. **Is the same value replaced the same way everywhere?** A hostname on its own, in a
   URL, in an email address. If one path differs, a reader can compare the two forms and
   the user loses consistency; if a path is weaker, it is the one that counts.

## The engine

`engine.rs`, `rule.rs`

- **Overlaps.** Candidates are sorted by start, then by length (longest first), then by
  rule order, and a candidate that overlaps an accepted one is skipped. The accepted
  range is recorded *before* the allowlist and the replacement are looked at. So a
  candidate that wins and is then not replaced (allowed, or its replacement equals the
  original, or its replacement would contain a line break) still shadows every candidate
  inside it. Look for a long candidate that ends up unreplaced over a short one that
  should have been replaced.
- **Nesting.** `Context::nested` runs the whole engine on a fragment. At `MAX_DEPTH` it
  returns the fragment unchanged. Build an input that reaches the limit (a URL in a query
  value of a URL, repeated) and look at what comes out in the clear at the bottom.
- **Dropped findings.** A finding is dropped when the replacement equals the original or
  contains `\n` or `\r`. Check that no rule can produce a line break for a detected value,
  because that would drop the finding and keep the original.
- **`render` with a filter.** The TUI lets the user keep single findings. Check that the
  default is "replace", that a kept finding is a visible choice, and that toggling state
  cannot carry over to another paste in a way that keeps a value the user never saw.
- **Rule order and the rule toggles.** A rule switched off in the config changes which
  candidates win. Check that switching one rule off does not uncover a value that another
  rule, still on, would have replaced as part of a larger match (a hostname inside a URL
  when `url = false`).

## Allowlist and terms

`config.rs`, `rules/term.rs`

- An allowed value is kept as a whole. Find out exactly what "as a whole" covers for a
  composite value: with `github.com` allowed, is `https://user:secret@github.com/x?token=abc`
  kept completely? What the README says and what the code does must agree, and the README
  must make the consequence clear.
- Wildcards and networks: `*.example.net` must not match `example.net.attacker.org` or
  `notexample.net`; a CIDR entry must not match across address families; `/0` allows
  everything and should be a deliberate act.
- An allowlist entry must not turn replacement off for values next to the allowed one.
- Terms match literally, ignoring case, as whole words. Check the word boundary for terms
  that start or end with a non-letter, for terms next to an apostrophe, a hyphen or a
  plural ending, and for other scripts. A term that is found in fewer places than the user
  expects is a G5 miss. Inflected forms and different spellings are a known limit of
  literal matching; check that the README says so.
- The replacement of a term keeps its shape. For a short or distinctive term (a product
  name with digits and punctuation) the shape may identify it among few candidates.

## The output as a whole

- **Structure that survives.** Which relations between originals are visible between
  replacements? Equal values (accepted), hostnames under a common parent (accepted), the
  suffix and the block (accepted). Must *not* survive: order (a smaller address stays
  smaller), closeness (two addresses in one /24 stay close), common prefixes of anything
  but hostnames from the right. A prefix-preserving mode is on the backlog; if it arrives,
  the budget changes and needs its own review.
- **Across kinds.** The same host in a URL and in an email address is linked on purpose.
  Check that nothing else links kinds: an IPv4 address and the same address IPv4-mapped in
  IPv6 map consistently on purpose; a phone number and a card number that share digits
  must not.
- **Across versions.** The same key gives the same replacements in every version. A
  change that alters any output is a breaking change (see "Constraints on fixes" in
  `guarantees.md`). The golden vector tests are what notices it: check that the kinds and
  paths in the scope have vectors.
- **Obfuscating twice.** Copying part of an obfuscated text during a session obfuscates it
  again. Check that the second pass cannot give the original back: a mapping that is its
  own inverse would. A keyed bijection applied twice is fine.

## Cost

Slow input is a security problem in session mode: while `obfuscate` runs, the original is
on the clipboard (G6). In the TUI and the filter it is only a nuisance.

- The rules are meant to be linear in the length of the text. Measure only where a loop
  in the scope gives a reason to doubt it.
- The `regex` crate matches in linear time. The code around it may not: a loop that
  rescans the text for every match, `chars().count()` or `filter().count()` inside a
  loop, `permute_within` when the member set is a small part of the range, nesting that
  re-runs every rule on a fragment at each level.
- For each loop in the scope, write down its cost in the length of the input and build
  the input that is worst for it. Time it with `probe.sh --release --summary` and a large
  input; a debug build is slower by a large factor and says little.
- Memory: a text is read completely before it is processed. Check what a very large
  selection does to `sagiri-session`.

## Probing

`scripts/probe.sh` runs text through the real filter with the published test key and an
empty or given config, isolated from the user's key, config and clipboard. It prints each
line as `kept` or `changed`, so a miss is visible at a glance. `--only <kind>` switches
the other rules off and shows which rule is responsible for a replacement; `--kept` and
`--summary` shorten the output of a large probe.

```bash
S=.claude/skills/sagiri-security-review/scripts

$S/probe.sh <<'EOF'
gateway 10.20.30.40 and HOST_10.20.30.40
mail jane.doe@acme-corp.com, card 4111 1111 1111 1111
https://deploy:hunter2@build.acme-corp.com:8443/jobs?token=abc123#top
EOF

# With an allowlist and terms:
T="$(mktemp -d)"
printf 'allow = ["acme-corp.com"]\nterms = ["Project Falcon"]\n' >"$T/config.toml"
echo 'Project Falcon runs on https://admin:pw@acme-corp.com/x?key=abc' | $S/probe.sh --config "$T/config.toml"
```

Rules for probes:

- Fictional data only, as in the tests: documentation address ranges (`192.0.2.0/24`,
  `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`), any address in a private,
  link-local or unique-local block, made-up names under real suffixes (`acme-corp.com`),
  published test card numbers. Names under `example.com`,
  `.test` or `.invalid` are kept by design and therefore useless for a hostname probe.
- One line per case, and one idea per line, so that `kept` means one thing.
- A probe that shows a problem goes into the report with its exact input and output. With
  the test key anyone can reproduce it.
- For experiments below the binary (counting collisions, timing a primitive, trying every
  value of a small permutation), write a throwaway test in a scratch directory outside the
  repository, with a `Cargo.toml` that depends on `sagiri-core` by path. Do not add files
  to the repository during a review.
