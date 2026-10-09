use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identity of a media item, timeline, track or clip. Diff and merge match by `Id`, never by position.
///
/// Ids are chosen by whoever *creates* an operation, not by `apply`, so applying an op is a pure function.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(Uuid);

impl Id {
    /// A fresh random id.
    pub fn new() -> Id {
        Id(Uuid::new_v4())
    }

    /// All 32 hex digits, for file names and logs where the short form could collide.
    pub fn to_full_string(self) -> String {
        self.0.simple().to_string()
    }

    /// Deterministic id for tests and fixtures.
    pub fn from_u128(v: u128) -> Id {
        Id(Uuid::from_u128(v))
    }
}

impl Default for Id {
    fn default() -> Self {
        Id::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // short form is enough for humans; full value is in the serialized form
        write!(f, "{}", &self.0.simple().to_string()[..8])
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_print_short_or_full() {
        let (a, b) = (Id::new(), Id::default());
        assert_ne!(a, b);
        let x = Id::from_u128(0xabcdef01_u128 << 96);
        assert_eq!(x.to_string(), "abcdef01");
        assert_eq!(x.to_full_string().len(), 32);
        assert_eq!(format!("{x:?}"), "Id(abcdef01)");
        assert!(x.to_full_string().starts_with("abcdef01"));
    }

    #[test]
    fn serializes_as_a_plain_uuid_string() {
        let x = Id::from_u128(1);
        let j = serde_json::to_string(&x).unwrap();
        assert_eq!(j, "\"00000000-0000-0000-0000-000000000001\"");
        assert_eq!(serde_json::from_str::<Id>(&j).unwrap(), x);
    }
}
