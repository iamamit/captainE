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
