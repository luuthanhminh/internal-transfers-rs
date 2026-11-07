# ADR 0002: Money as exact decimals, transported as strings

Status: accepted

## Context

Amounts can have many decimal places and must never drift or be rounded.
Several common choices fail silently:

- `f64` cannot represent `0.1` exactly (`0.1 + 0.2 != 0.3`).
- JSON numbers are parsed as floats by many clients and servers before our code
  sees them.
- Lenient decimal parsers accept `"1e5"`, `"+5"`, `".5"`, `"5."`, `"1_000"`.
- PostgreSQL `NUMERIC(p, s)` silently rounds values with more than `s`
  decimal places on insert.
- Fixed-precision decimal types overflow when large balances are read back.

## Decision

- Amounts are JSON **strings** in plain decimal notation. JSON numbers are
  rejected by the request types.
- A `Money` domain type parses with a strict grammar — digits, optional
  fractional part, no sign, exponent, leading zeros or whitespace — before
  handing the string to `BigDecimal`. Invalid values cannot be constructed.
- Limits: at most 20 integer digits and 18 decimal places. Values outside the
  limits are rejected with `422 invalid_amount`, never rounded.
- A balance stays below 10^20 for the same reason: a transfer that would push
  the destination to 10^20 or more is rejected with `422 balance_limit_exceeded`,
  backed by `CHECK (balance < 1e20)`.
- Storage is unconstrained PostgreSQL `NUMERIC`; precision is enforced by the
  application, so the database never rounds.
- Responses render the canonical form without trailing zeros (`"49.6"`).

## Consequences

- No float ever touches an amount; arithmetic in the database is exact.
- `bigdecimal` (arbitrary precision) is used instead of a fixed 96-bit decimal
  so that sums of large balances cannot overflow when decoded.
- Clients must send strings; this is documented and enforced with a clear error.

## Alternatives considered

- **Integer minor units (cents)**: exact and fast, but the number of decimal
  places differs per asset and was not specified; a fixed scale would either
  round or reject legitimate values.
- **`rust_decimal`**: 28 significant digits, fewer than 20 + 18, and it rounds
  silently beyond that.
