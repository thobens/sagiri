---
id: "0008"
title: Payment card and IBAN rules
status: done
depends_on: ["0003"]
---

# 0008 — Payment card and IBAN rules

## Why

Card numbers fall under PCI DSS and bank account numbers are personal financial data.
Neither may leave the machine in a pasted text. Both carry checksums, which makes detection
reliable and lets the replacement stay valid for anything that validates the format
downstream.

## Scope

`crates/sagiri-core/src/rules/card.rs` (`PaymentCardRule`) and `rules/iban.rs`
(`IbanRule`), both registered in `default_rules()` at their pinned positions.

### Payment cards

- **Detection.** 13–19 digits, either contiguous or in groups separated by single spaces or
  single hyphens (one separator style per number).
  - The character before and after the match is not alphanumeric.
  - The match is not directly preceded or followed by `.` or `,` plus a digit (it is not
    part of a decimal number).
  - The digits pass the Luhn check.
  - Prefix and length match one brand:

    | Brand | Prefix | Length |
    |---|---|---|
    | Visa | 4 | 13, 16, 19 |
    | Mastercard | 51–55, 2221–2720 | 16 |
    | American Express | 34, 37 | 15 |
    | Discover | 6011, 644–649, 65 | 16–19 |
    | Diners Club | 300–305, 36, 38, 39 | 14–19 |
    | JCB | 3528–3589 | 16–19 |
    | UnionPay | 62 | 16–19 |
    | Maestro | 5018, 5020, 5038, 5893, 6304, 6759, 6761, 6762, 6763 | 13–19 |

- **Replacement.**
  - Keep the brand prefix that matched (for example `4`, `37`, `6011`).
  - Draw the remaining digits except the last from `stream("card", digits)`.
  - Set the last digit so the number passes the Luhn check.
  - Put the separators back at their original positions.
- **Canonical form:** the digits only. The same number written with spaces, hyphens or
  neither gets the same digits.

### IBAN

- **Detection.** Two letters, two check digits, then the BBAN (letters and digits), either
  contiguous or in groups of four separated by single spaces (the last group may be
  shorter). All letters upper case, or all lower case.
  - The character before and after the match is not alphanumeric.
  - The country code is in the IBAN registry and the total length is the one the registry
    defines for it.
  - The mod-97 check gives 1.
- **Replacement.**
  - Keep the country code.
  - Replace every BBAN character with one of the same class (digit for digit, letter for
    letter), drawn from `stream("iban", canonical)`.
  - Recompute the two check digits.
  - Put spaces and letter case back as in the original.
- **Canonical form:** upper case without spaces.
- **Country table.** Embed the country-code-to-length table from the SWIFT IBAN registry.
  A maintained crate may be used for validation instead if it adds no heavy dependencies;
  say which option was taken in the report.

## Out of scope

- Card expiry dates, CVV codes, cardholder names.
- Other account number formats (US routing numbers, UK sort codes, BIC).
- Preserving the bank code inside the IBAN.
- Cards without a recognized brand prefix.

## Design notes

- **Luhn alone is not enough.** One in ten random digit strings passes it. Requiring a
  brand prefix and length cuts false positives on order numbers, timestamps and IDs.
- **The replacement is checksum-valid** so that the text keeps "looking right" to a reader
  or a validating tool. It is still random: any resemblance to a real card or account is
  coincidence.
- **The brand prefix is kept** because it carries meaning ("an Amex card") without
  identifying anyone. The rest of the issuer number is not kept.
- **The whole BBAN is replaced**, including the bank code: a bank code plus a country
  narrows an account down considerably.
- Published test numbers are replaced like any other number. There is no keep-list.
- **Card grouping.** In the grouped form the first group has exactly 4 digits and every
  later group 3-6 digits (4-4-4-4, 4-6-5, 4-6-4, 4-4-4-4-3, 4-4-5), joined by one separator
  style. The candidate is the maximal run of qualifying groups; no shorter prefix and no
  later start group is tried, so a run that is not a valid card as a whole is not detected
  (`4111 1111 1111 1111 1111`, `1234 4111 1111 1111 1111`). A hyphen joins tokens, a space
  separates them: a number directly preceded or followed by `-<digit>` (or `<digit>-`) is
  not detected (`4111111111111111-5`, `5-4111111111111111`), but one next to a space plus a
  digit is (`4111111111111111 12/25` and `5 4111111111111111` detect the card only).
  `4111 1111 1111 1111 12/25` is detected without `12`, which is too short to join the run.

## Acceptance criteria

Cards:

- [ ] Detected and replaced: `4111 1111 1111 1111`, `4111-1111-1111-1111`,
      `4111111111111111`, `5555555555554444`, `378282246310005`, `3782 822463 10005`,
      `6011111111111117`, `30569309025904`, `3530111333300000`.
- [ ] Every replacement passes the Luhn check, has the same length, the same brand prefix
      and the same separator positions as the original.
- [ ] `4111 1111 1111 1111`, `4111-1111-1111-1111` and `4111111111111111` get the same
      digits.
- [ ] Not detected: `4111 1111 1111 1112` (Luhn fails), `1234567890123456` (no brand),
      `4111  1111 1111 1111` (double space), `12.4111111111111111`,
      `A4111111111111111`, a 20-digit number, `1699999999999` (13-digit timestamp).
- [ ] Property test: for random Luhn-valid numbers with a valid brand prefix, the
      replacement is valid and differs from the original in at least one digit.

IBAN:

- [ ] Detected and replaced: `DE89 3704 0044 0532 0130 00`, `DE89370400440532013000`,
      `GB82 WEST 1234 5698 7654 32`, `CH93 0076 2011 6238 5295 7`,
      `FR14 2004 1010 0505 0001 3M02 606`, `NL91 ABNA 0417 1643 00`,
      `de89370400440532013000`.
- [ ] Every replacement keeps the country code, length, grouping, letter case and the
      class of each BBAN character, and passes the mod-97 check.
- [ ] The spaced and the contiguous form of the same IBAN get the same characters.
- [ ] Not detected: `DE89 3704 0044 0532 0130 01` (checksum fails),
      `DE8937040044053201300` (wrong length), `XX89370400440532013000` (unknown country),
      `De89370400440532013000` (mixed case).

Both:

- [ ] Golden vectors with the test key: three card numbers and three IBANs, as literals.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests and one property test per rule.
- `crates/sagiri-core/tests/golden_payment.rs` for the golden vectors through `Engine` with
  `default_rules()`.
