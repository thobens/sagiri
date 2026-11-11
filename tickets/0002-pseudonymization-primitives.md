---
id: "0002"
title: Keyed pseudonymization primitives and key file
status: todo
depends_on: ["0001"]
---

# 0002 — Keyed pseudonymization primitives and key file

## Why

Sagiri's central promise is that the same original always yields the same replacement
without storing any original. That requires a secret key and a small set of deterministic
building blocks that every rule uses. Their output becomes a compatibility contract, so they
are built and pinned before any rule exists.

## Scope

All in `crates/sagiri-core`.

### `paths.rs` — XDG paths

- `config_dir() -> Result<PathBuf, PathError>`: `$XDG_CONFIG_HOME/sagiri`, default
  `$HOME/.config/sagiri`.
- `data_dir() -> Result<PathBuf, PathError>`: `$XDG_DATA_HOME/sagiri`, default
  `$HOME/.local/share/sagiri`.
- An unset, empty or relative `XDG_*` value is ignored and the default is used.
- `HOME` unset or empty while it is needed: `PathError::NoHome`.
- Each function has a `_with` variant that takes the environment as a lookup closure
  (`&dyn Fn(&str) -> Option<OsString>`). The plain functions pass `std::env::var_os`.
  Implement by hand; do not add a directories crate.

### `key.rs` — secret key and key file

- `Key`: 32 bytes. Zeroized on drop. `Debug` prints `Key(<redacted>)`. No `Display`,
  no `Clone` exposed publicly, no serde.
- `Key::generate() -> Result<Key, KeyError>`: bytes from the operating system (`getrandom`).
- `Key::from_bytes([u8; 32]) -> Key`.
- `Key::load_or_create(path: &Path) -> Result<Key, KeyError>`:
  - File format: 64 lowercase hex characters followed by one `\n`. Anything else is
    `KeyError::Malformed` (the message names the path, never the content).
  - If the file is missing: create parent directories (mode 0700), create the file with
    `create_new` and mode 0600, write a fresh key. If creation fails with `AlreadyExists`
    (another process won the race), load the existing file instead.
  - If the file exists and any group or other permission bit is set:
    `KeyError::InsecurePermissions`. The message names the path and suggests `chmod 600`.
- `resolve_path(flag: Option<&Path>, env: &dyn Fn(&str) -> Option<OsString>)
  -> Result<PathBuf, PathError>`: the flag if given, else `SAGIRI_KEY_FILE` if set and
  non-empty, else `data_dir_with(env)/key`.

### `pseudo.rs` — deterministic primitives

Implement `Pseudonymizer` and `DetStream` with the signatures pinned in
`tickets/README.md`. `Pseudonymizer` keeps its own copy of the key and zeroizes it on drop.

- **`stream(domain, input)`**. Block `i` (counting from 0) is
  `HMAC-SHA256(key, "sagiri/v1" ‖ 0x00 ‖ domain ‖ 0x00 ‖ input ‖ be32(i))`.
  The stream is the concatenation of the blocks. `domain` must not contain a NUL byte
  (panic: it is a programming error).
  - `fill` copies the next bytes of the stream.
  - `next_u64` reads 8 bytes, big-endian.
  - `below(n)` returns a uniform value in `0..n` by rejection sampling over `next_u64`
    (no modulo bias). `n == 0` panics.
  - `pick(items)` is `&items[below(len)]`. An empty slice panics.
- **`permute(domain, bits, value)`**: a keyed bijection on `0..2^bits`, `2 <= bits <= 64`,
  `value < 2^bits` (otherwise panic).
  - Even `bits`: balanced Feistel network, 10 rounds. The round function is the first
    `bits/2` bits of
    `HMAC-SHA256(key, "sagiri/v1" ‖ 0x00 ‖ domain ‖ 0x00 ‖ "feistel" ‖ round as u8 ‖ bits as u8 ‖ be64(half))`.
  - Odd `bits`: run the even construction for `bits + 1` and repeat until the result is
    below `2^bits` (cycle-walking).
- **`permute_within(domain, bits, value, member)`**: precondition `member(value)`.
  Apply `permute` repeatedly until the result satisfies `member`. This is a bijection on
  the member subset.
- **`mimic(domain, seed, shape)`**: returns a string with the same number of characters as
  `shape`, drawing from `stream(domain, seed)`.
  - A character that is alphabetic becomes an ASCII letter, upper case if the original
    character is upper case, otherwise lower case.
  - A character that is numeric becomes an ASCII digit.
  - Every other character is copied unchanged.
  - Within a run of consecutive letters, consonants and vowels alternate, so the result is
    pronounceable. Consonants: `bdfgklmnprstvz`. Vowels: `aeiou`. Whether a run starts with
    a consonant or a vowel is drawn from the stream.
  - Which letter or digit is chosen depends only on the stream and the character position,
    never on the case of `shape`. So `mimic(d, s, "ACME")` equals
    `mimic(d, s, "acme").to_uppercase()`.
  - If the result equals `shape` and `shape` contains at least one letter or digit, derive
    again with a retry counter appended to the seed until it differs. A value is never
    mapped to itself.

## Out of scope

- Rules, the engine, any text scanning (tickets 0003 and later).
- The `--key-file` command-line flag (ticket 0005).
- Key rotation commands, several keys, passphrase-protected keys.
- Windows paths and permissions.

## Design notes

- **Why not the `rand` crate.** Its distributions may change between versions, which would
  silently change every replacement. The constructions above are fully specified here and
  locked by golden vectors.
- **Why a permutation as well as a stream.** In a small value space such as
  `192.168.0.0/16`, hashing to a random value makes collisions likely (two different
  addresses becoming the same one). A bijection cannot collide.
- **Why the environment is a parameter.** In edition 2024 `std::env::set_var` is `unsafe`,
  and the workspace forbids `unsafe`. Tests pass a closure instead of mutating the process
  environment.
- **Why hex in the key file.** It can be copied to another machine by hand to get the same
  replacements there.
- The key lives in the data directory, not the config directory; see "Files Sagiri touches"
  in `tickets/README.md`.

## Acceptance criteria

- [ ] `paths`: tests cover, for both directories: variable set to an absolute path;
      unset; empty; relative; `HOME` missing. No test calls `std::env::set_var`.
- [ ] `Key::load_or_create` on a missing path creates the file with mode 0600 and parent
      directories with mode 0700; a second call returns the same key bytes.
- [ ] A key file with mode 0644 is rejected with `InsecurePermissions`.
- [ ] Key files that are too short, too long, contain upper-case hex or non-hex characters
      are rejected with `Malformed`. The error text does not contain the file content.
- [ ] `format!("{:?}", key)` does not contain any key byte in hex.
- [ ] `resolve_path` honours the order flag, `SAGIRI_KEY_FILE`, XDG default.
- [ ] `stream`: same (key, domain, input) gives the same bytes; changing the key, the
      domain or the input gives different bytes; reading in pieces equals reading at once.
- [ ] `below(n)` always returns a value below `n` (property test over random `n`), and over
      60 000 draws with `n = 6` every bucket is within 5 % of the mean.
- [ ] `permute` is a bijection: exhaustive check for `bits` in {2, 3, 8, 11, 16}.
- [ ] `permute_within` is a bijection on the subset: exhaustive check for 16 bits with
      `member = |v| v % 3 != 0`.
- [ ] `mimic`: length in characters and character classes are preserved (property test over
      arbitrary strings, including non-ASCII); the case property above holds; the result
      differs from `shape` whenever `shape` has a letter or digit.
- [ ] Golden vectors with the test key (`0x00..=0x1f`): at least 3 `stream` outputs
      (first 16 bytes each), 5 `permute` values, 5 `mimic` strings, written as literals.
- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests next to each module; property tests with `proptest`; file tests with
  `tempfile`.
- Golden vectors in `crates/sagiri-core/tests/golden_primitives.rs`. Generate the expected
  values once from the finished implementation, check them by eye for plausibility, then
  commit them as literals.
