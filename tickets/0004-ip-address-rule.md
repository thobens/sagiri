---
id: "0004"
title: IP address rules (IPv4 + IPv6)
status: todo
depends_on: ["0003"]
---

# 0004 — IP address rules (IPv4 + IPv6)

## Why

IP addresses are the first kind of sensitive value Sagiri handles and the reference example
of the project: `172.55.0.1` is replaced by some other address, and by the same one every
time. This is also the first real rule, so it proves the engine and the primitives.

## Scope

`crates/sagiri-core/src/rules/ip.rs` with `Ipv4Rule` and `Ipv6Rule`, both registered in
`default_rules()` at their pinned positions.

### IPv4

- **Detection.** Four decimal octets separated by dots, each 0–255. Leading zeros are
  accepted and read as decimal (`010.001.000.001` is `10.1.0.1`).
  - The character before the match is not alphanumeric, `_` or `.`.
  - The character after the match is not alphanumeric or `_`, and is not a `.` followed by
    an alphanumeric character.
  - A following `:port` or `/prefix` is not part of the match and stays as it is.
- **Never replaced:** `0.0.0.0/8`, `127.0.0.0/8`, `224.0.0.0/4`, `240.0.0.0/4`. This covers
  the unspecified address, loopback, multicast, broadcast and every netmask (`255.x.x.x`).
- **Mapping.** `pub fn pseudonymize_v4(addr: Ipv4Addr, p: &Pseudonymizer) -> Ipv4Addr`.
  The category is preserved and the mapping is collision-free within it:

  | Category | Kept prefix | `permute` domain | Bits |
  |---|---|---|---|
  | `10.0.0.0/8` | 8 bits | `ipv4/10` | 24 |
  | `172.16.0.0/12` | 12 bits | `ipv4/172.16` | 20 |
  | `192.168.0.0/16` | 16 bits | `ipv4/192.168` | 16 |
  | `169.254.0.0/16` | 16 bits | `ipv4/169.254` | 16 |
  | `100.64.0.0/10` | 10 bits | `ipv4/100.64` | 22 |
  | public (everything else that is not "never replaced") | – | `ipv4/public` | 32, via `permute_within` with "is public" as the member test |

- **Output format:** plain dotted decimal without leading zeros.

### IPv6

- **Detection.** Find candidates made of hex groups and colons, including `::` and a
  trailing embedded IPv4 address. Accept a candidate only if
  `std::net::Ipv6Addr::from_str` parses it.
  - The character before the match is not alphanumeric, `:` or `.`.
  - The character after the match is not alphanumeric or `:`, and is not a `.` followed by
    an alphanumeric character.
  - Square brackets, a `%zone` suffix, a `/prefix` and a `:port` after `]` are not part of
    the match.
- **Never replaced:** `::`, `::1`, `ff00::/8`.
- **Mapping.** `pub fn pseudonymize_v6(addr: Ipv6Addr, p: &Pseudonymizer) -> Ipv6Addr`:

  | Category | Kept | Rest |
  |---|---|---|
  | IPv4-mapped `::ffff:a.b.c.d` | the `::ffff:` prefix | embedded address through `pseudonymize_v4` (unchanged if that one is never replaced) |
  | Link-local `fe80::/10` | first 64 bits | low 64 bits from `stream("ipv6/link-local", addr bytes)` |
  | Unique local `fc00::/7` | first 8 bits | 120 bits from `stream("ipv6/ula", addr bytes)` |
  | Global `2000::/3` | first 3 bits | 125 bits from `stream("ipv6/global", addr bytes)` |
  | Anything else | first 16 bits | 112 bits from `stream("ipv6/other", addr bytes)` |

- **Output format:** the `Display` form of `Ipv6Addr` (lower case, compressed). An
  IPv4-mapped address keeps its `::ffff:a.b.c.d` form.

### Canonical form

The parsed address. Different spellings of one address give the same replacement.

## Out of scope

- Prefix-preserving mapping and CIDR-aware handling of network addresses (backlog).
- MAC addresses (backlog).
- Dashed forms such as `ip-10-0-0-1`.
- Telling version numbers like `1.2.3.4` apart from addresses. See design notes.

## Design notes

- **Category preservation.** A private address stays private and in the same block, a
  public one stays public. The text keeps its meaning ("this is an internal host") while
  the value changes. `172.55.0.1` is public (the private block is `172.16.0.0/12`), so it
  maps to some public address.
- **Why a permutation for IPv4.** Hashing into `192.168.0.0/16` would make two different
  addresses collide with noticeable probability, and the obfuscated text would then claim
  two hosts are one. IPv6 has enough room that derived bits cannot realistically collide.
- **Fixed points.** A permutation can map an address to itself with probability
  1 / (size of the category). This is accepted and not worked around.
- **Known false positive.** A bare four-part version number (`1.2.3.4`) is
  indistinguishable from an address and will be replaced. A leading `v` or a longer dotted
  sequence prevents the match. The user can switch a finding off in the TUI (ticket 0011).
- The `regex` crate has no look-around: check the neighbouring characters in code.

## Acceptance criteria

- [ ] Reference example: a text containing `172.55.0.1` twice gets the same replacement in
      both places; a second `Engine` built from the same key gives the same replacement; a
      different key gives a different one. The replacement is a public address.
- [ ] Category table test: one address per category maps into the same category.
- [ ] `ipv4_192_168_is_bijective`: all 65 536 addresses of `192.168.0.0/16` map to 65 536
      distinct addresses inside the block.
- [ ] 100 000 sample addresses from `10.0.0.0/8` and 100 000 public addresses map without
      collision and stay in their category.
- [ ] Never-replaced addresses are left untouched: `0.0.0.0`, `127.0.0.1`,
      `255.255.255.0`, `255.255.255.255`, `224.0.0.251`, `::`, `::1`, `ff02::1`.
- [ ] Detection table (must match the address only): `10.0.0.1`, `(10.0.0.1)`,
      `10.0.0.1:8080`, `10.0.0.0/8`, `ip=10.0.0.1,`, `010.001.000.001`,
      `[2001:db8::1]:443`, `fe80::1%eth0`, `2001:db8::/32`, `::ffff:192.168.1.10`.
- [ ] Rejection table (no finding): `256.1.1.1`, `1.2.3.4.5`, `v1.2.3.4`,
      `libfoo-1.2.3.4.tar.gz`, `12:34:56`, `aa:bb:cc:dd:ee:ff`, `1.2.3`, `x10.0.0.1`.
- [ ] `010.001.000.001` and `10.1.0.1` get the same replacement.
- [ ] `2001:DB8::1` and `2001:db8:0:0:0:0:0:1` get the same replacement.
- [ ] `::ffff:192.168.1.10` maps to `::ffff:` followed by exactly what `192.168.1.10` maps
      to.
- [ ] Golden vectors with the test key: at least one address per IPv4 category and per IPv6
      category, as literals.
- [ ] Text around the findings is unchanged byte for byte.
- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests in `rules/ip.rs` for detection and mapping tables.
- `crates/sagiri-core/tests/golden_ip.rs` for the golden vectors and the reference example
  through `Engine` with `default_rules()`.
