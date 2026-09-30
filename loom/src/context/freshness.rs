//! How current a derived retrieval layer is.
//!
//! Split out of [`crate::context::schema`] so the shared type contract stays
//! readable; [`crate::context::schema`] re-exports [`Freshness`], so every
//! existing import path still resolves. The values themselves are produced by
//! [`crate::context::refresh`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How current a derived layer is relative to the bytes it was built from.
///
/// A pack carries two: `structural` (the chunk catalog vs the markdown on disk)
/// and `semantic` (the source graph vs the tracked source tree). They move
/// independently — editing a `.rs` file staleness the semantic layer while the
/// knowledge catalog stays perfectly fresh.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    /// Content revision the layer was built from (hex sha256, or empty if never built).
    #[serde(default)]
    pub revision: String,
    /// When the layer was last rebuilt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed_at: Option<DateTime<Utc>>,
    /// True when the on-disk inputs no longer match `revision`.
    #[serde(default)]
    pub stale: bool,
    /// Human-readable cause when `stale` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// True when the project has no resolvable git HEAD to compare against.
    /// Derived on every read, never persisted.
    #[serde(skip)]
    pub unavailable: bool,
}

/// The four states a source graph can be in, on every surface that reports one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphState {
    Current,
    Stale,
    NeverBuilt,
    Unavailable,
}

impl GraphState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::NeverBuilt => "never built",
            Self::Unavailable => "unavailable",
        }
    }
}

impl Freshness {
    /// The state this record reports: `unavailable` wins, then an empty
    /// revision (never built), then `stale`.
    pub fn state(&self) -> GraphState {
        if self.unavailable {
            GraphState::Unavailable
        } else if self.revision.is_empty() {
            GraphState::NeverBuilt
        } else if self.stale {
            GraphState::Stale
        } else {
            GraphState::Current
        }
    }

    /// A layer that has never been built. Reported as stale so callers never
    /// mistake "no data" for "up to date".
    pub fn never_built(detail: impl Into<String>) -> Self {
        Freshness {
            revision: String::new(),
            computed_at: None,
            stale: true,
            detail: Some(detail.into()),
            unavailable: false,
        }
    }

    /// A layer that cannot be judged because the project has no resolvable git
    /// HEAD. Callers keep the stored `revision` on the returned value.
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Freshness {
            detail: Some(detail.into()),
            unavailable: true,
            ..Freshness::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(stale: bool) -> Freshness {
        Freshness {
            revision: "abc".into(),
            stale,
            ..Freshness::default()
        }
    }

    #[test]
    fn state_covers_all_four_states() {
        assert_eq!(built(false).state(), GraphState::Current);
        assert_eq!(built(true).state(), GraphState::Stale);
        assert_eq!(Freshness::never_built("x").state(), GraphState::NeverBuilt);
        assert_eq!(Freshness::unavailable("x").state(), GraphState::Unavailable);
    }

    #[test]
    fn unavailable_wins_over_a_kept_revision() {
        let kept = Freshness {
            unavailable: true,
            ..built(false)
        };
        assert_eq!(kept.state(), GraphState::Unavailable);
    }

    #[test]
    fn an_empty_revision_is_never_built_even_when_not_stale() {
        assert_eq!(Freshness::default().state(), GraphState::NeverBuilt);
    }

    #[test]
    fn state_strings_are_the_surface_words() {
        let words: Vec<_> = [
            GraphState::Current,
            GraphState::Stale,
            GraphState::NeverBuilt,
            GraphState::Unavailable,
        ]
        .into_iter()
        .map(GraphState::as_str)
        .collect();
        assert_eq!(words, ["current", "stale", "never built", "unavailable"]);
    }

    #[test]
    fn unavailable_is_never_persisted() {
        let json = serde_json::to_string(&Freshness::unavailable("no head")).unwrap();
        assert!(!json.contains("unavailable"), "{json}");
        let back: Freshness =
            serde_json::from_str(r#"{"revision":"abc","unavailable":true}"#).unwrap();
        assert!(!back.unavailable);
    }
}
