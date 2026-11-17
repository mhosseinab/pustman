//! Identity types shared by every Pustman crate.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self, Display};
use std::str::FromStr;

/// Stable identity of one collection item: a collection, folder or request.
///
/// Serializes as its canonical 26-character ULID string so saved workspaces stay
/// diffable and ids remain readable in JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(ulid::Ulid);

impl ItemId {
    /// Mint a new random identifier.
    pub fn new() -> Self {
        Self(ulid::Ulid::generate())
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromStr for ItemId {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ulid::Ulid::from_str(s).map(Self)
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ItemId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ItemId::from_str(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "api_model_tests.rs"]
mod tests;
