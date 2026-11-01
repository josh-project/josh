//! Forge-neutral change-state vocabulary.
//!
//! The per-change data a forge sync stores on a changes ref (`gh/` for
//! GitHub, `test/` for the test forge), plus the shared enums inside it.
//! Everything here is persisted through josh-git-serde: field names and enum
//! strings are the on-disk format and must stay byte-stable. Fields no forge
//! is obliged to have (PR number, URL, draft/merge state) are `Option` and
//! absent from the tree when `None`, so a forge without the concept stores
//! nothing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A label on a change. Fetch-time only; sequences are unsupported by the
/// git-tree format, so labels are never persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeLabel {
    pub name: String,
    pub color: String,
}

/// A comment on a change, as fetched from the forge. Fetch-time only, never
/// persisted (see [`ChangeData::comments`]).
#[derive(Debug, Clone)]
pub struct ChangeComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub timestamp: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub reply_to: Option<String>,
    pub commit_oid: Option<String>,
}

/// Latest state of a review of a change.
///
/// Serialized as a plain string (the snake_case variant name) rather than
/// via derived enum serde: the josh-git-serde tree format represents enum
/// variants as tree entries, and the union-merge writes on changes refs
/// cannot retract an old variant when the state changes. A string is a
/// same-named blob that updates overwrite cleanly. The JSON representation
/// is identical either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

impl ReviewState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewState::Approved => "approved",
            ReviewState::ChangesRequested => "changes_requested",
            ReviewState::Commented => "commented",
            ReviewState::Dismissed => "dismissed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "approved" => Some(ReviewState::Approved),
            "changes_requested" => Some(ReviewState::ChangesRequested),
            "commented" => Some(ReviewState::Commented),
            "dismissed" => Some(ReviewState::Dismissed),
            _ => None,
        }
    }
}

impl Serialize for ReviewState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReviewState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ReviewState::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown review state: {}", s)))
    }
}

/// State of a single check run, folding its execution status and conclusion
/// into one persisted value.
///
/// Serialized as a plain string, not a derived unit-variant tree entry, for
/// the same reason as [`ReviewState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// Queued or still running (no conclusion yet).
    Pending,
    Success,
    Failure,
    Neutral,
    Skipped,
    Cancelled,
    TimedOut,
    ActionRequired,
}

impl CheckState {
    /// Conservative admission rule: only an explicit success counts; neutral
    /// and skipped do not.
    pub fn passed(&self) -> bool {
        matches!(self, CheckState::Success)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CheckState::Pending => "pending",
            CheckState::Success => "success",
            CheckState::Failure => "failure",
            CheckState::Neutral => "neutral",
            CheckState::Skipped => "skipped",
            CheckState::Cancelled => "cancelled",
            CheckState::TimedOut => "timed_out",
            CheckState::ActionRequired => "action_required",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(CheckState::Pending),
            "success" => Some(CheckState::Success),
            "failure" => Some(CheckState::Failure),
            "neutral" => Some(CheckState::Neutral),
            "skipped" => Some(CheckState::Skipped),
            "cancelled" => Some(CheckState::Cancelled),
            "timed_out" => Some(CheckState::TimedOut),
            "action_required" => Some(CheckState::ActionRequired),
            _ => None,
        }
    }
}

impl Serialize for CheckState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CheckState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        CheckState::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown check state: {}", s)))
    }
}

/// A required status check on a target branch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequiredStatusCheck {
    pub context: String,
    pub integration_id: Option<i64>,
}

/// The forge-side state of one change, as stored on a changes ref by a forge
/// sync. GitHub fills every field; a forge with no PR concept (the test
/// forge) leaves the PR-specific fields `None`, and they are absent from the
/// stored tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeData {
    pub title: String,
    pub body: Option<String>,
    /// PR/MR number, for forges that have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<i64>,
    /// Web URL of the change on the forge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_by: Option<String>,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub base_ref_name: String,
    pub head_ref_name: String,
    /// Latest review state per reviewer login.
    #[serde(default)]
    pub reviews: BTreeMap<String, ReviewState>,
    /// Latest state per check-run name on the head commit.
    #[serde(default)]
    pub checks: BTreeMap<String, CheckState>,
    // Sequences are unsupported by the git-tree format; labels and comments
    // are fetch-time only and never persisted.
    #[serde(skip)]
    pub labels: Vec<ChangeLabel>,
    #[serde(skip)]
    pub comments: Vec<ChangeComment>,
}

impl ChangeData {
    /// Coarse review summary in the same shape as the old stored
    /// `review_decision` rollup ("Approved" / "ChangesRequested" /
    /// "ReviewRequired"), derived from the per-reviewer map. Dismissed
    /// reviews are ignored.
    pub fn review_decision_rollup(&self) -> Option<String> {
        let states = self
            .reviews
            .values()
            .filter(|s| !matches!(s, ReviewState::Dismissed));
        let mut saw_review = false;
        let mut saw_approved = false;
        for state in states {
            saw_review = true;
            match state {
                ReviewState::ChangesRequested => {
                    return Some("ChangesRequested".to_string());
                }
                ReviewState::Approved => saw_approved = true,
                _ => {}
            }
        }
        if saw_approved {
            Some("Approved".to_string())
        } else if saw_review {
            Some("ReviewRequired".to_string())
        } else {
            None
        }
    }

    /// Coarse check summary in the same shape as the old stored
    /// `check_status` rollup ("Success" / "Failure" / "Pending"), derived
    /// from the per-check map.
    pub fn check_status_rollup(&self) -> Option<String> {
        if self.checks.is_empty() {
            return None;
        }
        let mut saw_pending = false;
        for state in self.checks.values() {
            match state {
                CheckState::Pending => saw_pending = true,
                CheckState::Success | CheckState::Neutral | CheckState::Skipped => {}
                _ => return Some("Failure".to_string()),
            }
        }
        if saw_pending {
            Some("Pending".to_string())
        } else {
            Some("Success".to_string())
        }
    }
}
