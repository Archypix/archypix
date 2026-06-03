use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "tag_source", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TagSource {
    Manual,
    Rule,
    Segment,
    ShareMapping,
    IncomingShare,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub picture_id: Uuid,
    /// Stored as ltree text (dot-separated, e.g. `Photos.Travel.Alps`).
    pub tag_path: String,
    pub source: TagSource,
    pub source_id: Option<Uuid>,
    pub assigned_at: NaiveDateTime,
}

/// A validated, normalized tag path in ltree format (dot-separated labels).
///
/// Human-readable form uses slashes: `/Photos/Travel/Alps`
/// Stored ltree form uses dots: `Photos.Travel.Alps`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagPath(String);

impl TagPath {
    /// Parse from the human-readable slash-separated form (`/Photos/Travel/Alps`).
    pub fn from_slash(raw: &str) -> Self {
        let normalized = raw.trim().trim_start_matches('/').replace('/', ".");
        TagPath(normalized)
    }

    /// Wrap an already-normalized ltree string.
    pub fn from_ltree(ltree: impl Into<String>) -> Self {
        TagPath(ltree.into())
    }

    pub fn as_ltree(&self) -> &str {
        &self.0
    }

    /// Build the `/SharedToMe/<sender>/...` path for a federation-received tag.
    ///
    /// Sender identity is encoded as `{username}_AT_{instance}` where `.` becomes `_DOT_`,
    /// giving a reversible, unambiguous LTREE label. Example:
    /// `alice@instance.com` + `Photos.Travel` → `SharedToMe.alice_AT_instance_DOT_com.Photos.Travel`
    pub fn shared_to_me(sender_username: &str, sender_instance: &str, original: &TagPath) -> Self {
        let label = encode_sender_label(sender_username, sender_instance);
        if original.0.is_empty() {
            TagPath(format!("SharedToMe.{label}"))
        } else {
            TagPath(format!("SharedToMe.{label}.{}", original.0))
        }
    }

    /// All ancestor paths, from root down to (but not including) self.
    pub fn ancestors(&self) -> Vec<TagPath> {
        let parts: Vec<&str> = self.0.split('.').collect();
        (0..parts.len().saturating_sub(1))
            .map(|i| TagPath(parts[..=i].join(".")))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for TagPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for TagPath {
    fn from(s: String) -> Self {
        TagPath(s)
    }
}

impl From<&str> for TagPath {
    fn from(s: &str) -> Self {
        TagPath(s.to_string())
    }
}

/// Encode a sender identity (`username@instance`) as a single LTREE label.
///
/// `@` → `_AT_`, `.` → `_DOT_`, any remaining non-alphanumeric → `_`.
///
/// Usernames are restricted to `[a-z0-9_]` at registration, so only dots and the `_AT_`
/// separator need escaping. No collisions are possible within the username component.
fn encode_sender_label(username: &str, instance: &str) -> String {
    let encode = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                '.' => "_DOT_".to_string(),
                c if c.is_ascii_alphanumeric() || c == '_' => c.to_string(),
                _ => "_".to_string(),
            })
            .collect()
    };
    format!("{}_AT_{}", encode(username), encode(instance))
}
