use serde::Serialize;
use std::fmt;

/// A client-chosen account identifier. Always strictly positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AccountId(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("account_id must be a positive integer")]
pub struct InvalidAccountId;

impl AccountId {
    pub fn new(raw: i64) -> Result<Self, InvalidAccountId> {
        if raw > 0 {
            Ok(Self(raw))
        } else {
            Err(InvalidAccountId)
        }
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_positive_ids() {
        assert_eq!(AccountId::new(123).map(AccountId::get), Ok(123));
    }

    #[test]
    fn rejects_zero_and_negative_ids() {
        assert_eq!(AccountId::new(0), Err(InvalidAccountId));
        assert_eq!(AccountId::new(-1), Err(InvalidAccountId));
    }
}
