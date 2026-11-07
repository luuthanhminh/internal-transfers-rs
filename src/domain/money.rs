//! Monetary amounts.
//!
//! Amounts travel as JSON strings and are parsed with a strict grammar rather
//! than `BigDecimal::from_str`, which also accepts signs, exponents and other
//! forms (`"1e5"`, `"+5"`, `".5"`) that are ambiguous for money. Values are
//! never rounded: anything outside the supported precision is rejected.

use bigdecimal::{BigDecimal, Zero};
use std::str::FromStr;

/// Maximum digits before the decimal point (amounts below 10^20).
pub const MAX_INTEGER_DIGITS: usize = 20;
/// Maximum digits after the decimal point (enough for 18-decimal tokens).
pub const MAX_FRACTION_DIGITS: usize = 18;

/// A validated, non-negative amount of money.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money(BigDecimal);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error(
        "amount must be a decimal string such as \"100.25\": digits, an optional fractional part, no sign, exponent or leading zeros"
    )]
    Malformed,
    #[error("amount must have at most {MAX_INTEGER_DIGITS} digits before the decimal point")]
    TooLarge,
    #[error("amount must have at most {MAX_FRACTION_DIGITS} decimal places")]
    TooPrecise,
    #[error("amount must not be negative")]
    Negative,
    #[error("amount must be greater than zero")]
    NotPositive,
}

impl Money {
    /// Parses a non-negative amount (zero allowed), e.g. an opening balance.
    pub fn parse(input: &str) -> Result<Self, MoneyError> {
        // A well-formed number with a minus sign gets its own, clearer error.
        if let Some(magnitude) = input.strip_prefix('-')
            && Self::parse(magnitude).is_ok()
        {
            return Err(MoneyError::Negative);
        }

        let (integer, fraction) = match input.split_once('.') {
            Some((integer, fraction)) => (integer, Some(fraction)),
            None => (input, None),
        };

        if !is_digits(integer) || (integer.len() > 1 && integer.starts_with('0')) {
            return Err(MoneyError::Malformed);
        }
        if integer.len() > MAX_INTEGER_DIGITS {
            return Err(MoneyError::TooLarge);
        }
        if let Some(fraction) = fraction {
            if !is_digits(fraction) {
                return Err(MoneyError::Malformed);
            }
            if fraction.len() > MAX_FRACTION_DIGITS {
                return Err(MoneyError::TooPrecise);
            }
        }

        BigDecimal::from_str(input)
            .map(Self)
            .map_err(|_| MoneyError::Malformed)
    }

    /// Parses an amount that must be strictly greater than zero, e.g. a transfer.
    pub fn parse_positive(input: &str) -> Result<Self, MoneyError> {
        let money = Self::parse(input).map_err(|err| match err {
            MoneyError::Negative => MoneyError::NotPositive,
            other => other,
        })?;
        if money.0.is_zero() {
            return Err(MoneyError::NotPositive);
        }
        Ok(money)
    }

    pub fn as_decimal(&self) -> &BigDecimal {
        &self.0
    }
}

/// Exclusive upper bound for any balance: 10^20, the same bound as a single
/// amount, so every balance a client reads fits the documented 20 integer
/// digits. Mirrored by `CHECK (balance < 1e20)` in the database.
pub fn balance_limit() -> BigDecimal {
    BigDecimal::new(1.into(), -(MAX_INTEGER_DIGITS as i64))
}

/// Canonical string form used in every response: plain notation, no trailing
/// zeros (`"49.60"` and `"49.6"` both render as `"49.6"`, `"100.00"` as `"100"`).
pub fn format_amount(value: &BigDecimal) -> String {
    value.normalized().to_plain_string()
}

/// Non-empty and ASCII digits only.
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(input: &str) -> Result<String, MoneyError> {
        Money::parse(input).map(|m| format_amount(m.as_decimal()))
    }

    #[test]
    fn accepts_plain_decimal_strings() {
        assert_eq!(rendered("100.23344"), Ok("100.23344".into()));
        assert_eq!(rendered("0"), Ok("0".into()));
        assert_eq!(rendered("0.5"), Ok("0.5".into()));
    }

    #[test]
    fn rejects_forms_that_a_lenient_decimal_parser_would_accept() {
        for input in [
            "1_000", "1e5", "1E5", "+5", "--5", "-.5", ".5", "5.", "00012", "01.5",
        ] {
            assert_eq!(
                Money::parse(input),
                Err(MoneyError::Malformed),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn rejects_whitespace_empty_and_non_numeric_input() {
        for input in ["", " 1", "1 ", "NaN", "abc", "1.2.3", "１"] {
            assert_eq!(
                Money::parse(input),
                Err(MoneyError::Malformed),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn rejects_more_decimal_places_than_supported_instead_of_rounding() {
        let too_precise = format!("0.{}", "1".repeat(MAX_FRACTION_DIGITS + 1));
        assert_eq!(Money::parse(&too_precise), Err(MoneyError::TooPrecise));

        let max_precise = format!("0.{}", "1".repeat(MAX_FRACTION_DIGITS));
        assert_eq!(rendered(&max_precise), Ok(max_precise.clone()));
    }

    #[test]
    fn rejects_amounts_above_the_maximum() {
        let too_large = "9".repeat(MAX_INTEGER_DIGITS + 1);
        assert_eq!(Money::parse(&too_large), Err(MoneyError::TooLarge));
    }

    #[test]
    fn rejects_negative_amounts_with_a_dedicated_error() {
        assert_eq!(Money::parse("-5"), Err(MoneyError::Negative));
        assert_eq!(Money::parse("-0.01"), Err(MoneyError::Negative));
    }

    #[test]
    fn transfer_amount_must_be_greater_than_zero() {
        assert_eq!(Money::parse_positive("-1"), Err(MoneyError::NotPositive));
        assert_eq!(Money::parse_positive("0"), Err(MoneyError::NotPositive));
        assert_eq!(Money::parse_positive("0.000"), Err(MoneyError::NotPositive));
        assert!(Money::parse_positive("0.000000000000000001").is_ok());
    }

    #[test]
    fn keeps_exact_values_without_float_drift() {
        let sum = Money::parse("0.1").map(|a| a.0).unwrap_or_default()
            + Money::parse("0.2").map(|b| b.0).unwrap_or_default();
        assert_eq!(format_amount(&sum), "0.3");
    }

    #[test]
    fn the_balance_limit_is_one_above_the_largest_integer_part() {
        let limit = format!("1{}", "0".repeat(MAX_INTEGER_DIGITS));
        assert_eq!(format_amount(&balance_limit()), limit);
    }

    #[test]
    fn formats_balances_without_trailing_zeros_or_exponents() {
        assert_eq!(rendered("49.60"), Ok("49.6".into()));
        assert_eq!(rendered("100.00"), Ok("100".into()));
        assert_eq!(rendered("1000000"), Ok("1000000".into()));
        assert_eq!(
            rendered("0.000000000000000001"),
            Ok("0.000000000000000001".into())
        );
    }
}

/// Property-based tests: instead of hand-picked examples, generate thousands
/// of inputs and check rules that must hold for every one of them.
#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    /// Every spelling the documented grammar allows: `0` or no leading zero,
    /// at most 20 integer digits, an optional fraction of 1 to 18 digits.
    const VALID_AMOUNT: &str = "(0|[1-9][0-9]{0,19})(\\.[0-9]{1,18})?";

    /// The canonical rendering: plain digits, no exponent, no trailing zeros.
    fn is_canonical(rendered: &str) -> bool {
        let digits_and_point = rendered.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        let no_trailing_zero = !rendered.contains('.') || !rendered.ends_with(['0', '.']);
        digits_and_point && no_trailing_zero
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2_000))]

        #[test]
        fn every_valid_amount_is_accepted_exactly_and_round_trips(input in VALID_AMOUNT) {
            let money = Money::parse(&input).map_err(|e| TestCaseError::fail(format!("{input:?}: {e}")))?;
            let rendered = format_amount(money.as_decimal());
            prop_assert!(is_canonical(&rendered), "{input:?} rendered as {rendered:?}");
            // Never rounded: the rendered value is numerically the input, and
            // parsing the rendered form gives the same value back.
            let reparsed = Money::parse(&rendered).map_err(|e| TestCaseError::fail(format!("{rendered:?}: {e}")))?;
            prop_assert_eq!(reparsed.as_decimal(), money.as_decimal());
            prop_assert_eq!(BigDecimal::from_str(&input).ok(), Some(money.as_decimal().clone()));
        }

        #[test]
        fn any_input_is_handled_without_panicking(input in "\\PC{0,48}") {
            // Accepted or rejected, but never a panic; and anything accepted
            // obeys the same round trip as above.
            if let Ok(money) = Money::parse(&input) {
                let rendered = format_amount(money.as_decimal());
                prop_assert!(is_canonical(&rendered), "{input:?} rendered as {rendered:?}");
                prop_assert_eq!(BigDecimal::from_str(&input).ok(), Some(money.as_decimal().clone()));
            }
        }

        #[test]
        fn more_than_18_decimal_places_is_rejected_never_rounded(integer in "[1-9][0-9]{0,5}", fraction in "[0-9]{19,30}") {
            prop_assert_eq!(Money::parse(&format!("{integer}.{fraction}")), Err(MoneyError::TooPrecise));
        }

        #[test]
        fn a_minus_sign_is_always_a_negative_or_not_positive_error(input in VALID_AMOUNT) {
            let negative = format!("-{input}");
            prop_assert_eq!(Money::parse(&negative), Err(MoneyError::Negative));
            prop_assert_eq!(Money::parse_positive(&negative), Err(MoneyError::NotPositive));
        }

        #[test]
        fn a_transfer_amount_is_rejected_exactly_when_it_is_zero(input in VALID_AMOUNT) {
            let is_zero = Money::parse(&input).map(|m| m.as_decimal().is_zero()).unwrap_or(false);
            prop_assert_eq!(Money::parse_positive(&input).is_err(), is_zero);
        }
    }
}
