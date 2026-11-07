//! Validated domain types. Values of these types are guaranteed valid, so the
//! rest of the code never re-checks them.

mod account_id;
mod money;

pub use account_id::{AccountId, InvalidAccountId};
pub use money::{
    MAX_FRACTION_DIGITS, MAX_INTEGER_DIGITS, Money, MoneyError, balance_limit, format_amount,
};
