//! Folder paths on decks (DOMAIN_MODEL.md §3: folders are a path attribute,
//! not an entity).

use crate::error::{DomainError, FolderPathError};
use crate::limits::{MAX_FOLDER_PATH_BYTES, MAX_FOLDER_SEGMENT_BYTES, MAX_FOLDER_SEGMENTS};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Ordered folder path of a deck inside its workspace. Empty = workspace root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FolderPath(Vec<String>);

impl FolderPath {
    /// The workspace root (no folder).
    #[must_use]
    pub const fn root() -> Self {
        Self(Vec::new())
    }

    /// Parses a `/`-separated path; `""` is the root.
    ///
    /// v1 rules (fail closed): segments are non-empty, not `.`/`..`, free of
    /// leading/trailing whitespace, at most [`MAX_FOLDER_SEGMENT_BYTES`] UTF-8
    /// bytes, and contain no `\`, `/`, or C0/C1 control characters; at most
    /// [`MAX_FOLDER_SEGMENTS`] segments whose join stays within
    /// [`MAX_FOLDER_PATH_BYTES`] bytes.
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        if s.len() > MAX_FOLDER_PATH_BYTES {
            return Err(DomainError::InvalidFolderPath {
                reason: FolderPathError::PathTooLong,
            });
        }
        if s.is_empty() {
            return Ok(Self::root());
        }
        let mut segments = Vec::new();
        for segment in s.split('/') {
            if segment.is_empty() {
                return Err(DomainError::InvalidFolderPath {
                    reason: FolderPathError::EmptySegment,
                });
            }
            if segment.trim().is_empty() || segment.trim() != segment {
                return Err(DomainError::InvalidFolderPath {
                    reason: FolderPathError::SegmentWhitespace,
                });
            }
            if segment == "." || segment == ".." {
                return Err(DomainError::InvalidFolderPath {
                    reason: FolderPathError::DotSegment,
                });
            }
            if segment.len() > MAX_FOLDER_SEGMENT_BYTES {
                return Err(DomainError::InvalidFolderPath {
                    reason: FolderPathError::SegmentTooLong,
                });
            }
            for ch in segment.chars() {
                if matches!(ch, '\\' | '\u{7f}') || ch.is_control() {
                    return Err(DomainError::InvalidFolderPath {
                        reason: FolderPathError::ForbiddenCharacter,
                    });
                }
            }
            segments.push(segment.to_owned());
        }
        if segments.len() > MAX_FOLDER_SEGMENTS {
            return Err(DomainError::InvalidFolderPath {
                reason: FolderPathError::TooManySegments,
            });
        }
        let joined = segments.join("/");
        if joined.len() > MAX_FOLDER_PATH_BYTES {
            return Err(DomainError::InvalidFolderPath {
                reason: FolderPathError::PathTooLong,
            });
        }
        Ok(Self(segments))
    }

    /// Canonical serialized form: segments joined by `/`; root serializes to
    /// the empty string.
    #[must_use]
    pub fn as_path_string(&self) -> String {
        self.0.join("/")
    }

    /// Immutable segment view.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// True when this is the workspace root.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for FolderPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_path_string())
    }
}

impl Serialize for FolderPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.as_path_string())
    }
}

impl<'de> Deserialize<'de> for FolderPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::FolderPath;
    use crate::error::{DomainError, FolderPathError};
    use crate::limits::{MAX_FOLDER_PATH_BYTES, MAX_FOLDER_SEGMENT_BYTES, MAX_FOLDER_SEGMENTS};

    fn err_of(s: &str) -> FolderPathError {
        match FolderPath::parse(s) {
            Err(DomainError::InvalidFolderPath { reason }) => reason,
            other => panic!("expected InvalidFolderPath for {s:?}, got {other:?}"),
        }
    }

    #[test]
    fn root_serializes_to_empty_string() {
        let root = FolderPath::root();
        assert!(root.is_root());
        assert_eq!(root.as_path_string(), "");
        assert_eq!(FolderPath::parse("").unwrap(), root);
        let json = serde_json::to_string(&root).unwrap();
        assert_eq!(json, "\"\"");
        let back: FolderPath = serde_json::from_str(&json).unwrap();
        assert_eq!(back, root);
    }

    #[test]
    fn segments_round_trip_canonically() {
        let path = FolderPath::parse("streaming/overlays").unwrap();
        assert_eq!(path.segments(), ["streaming", "overlays"]);
        assert_eq!(path.as_path_string(), "streaming/overlays");
        assert!(!path.is_root());
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(json, "\"streaming/overlays\"");
        let back: FolderPath = serde_json::from_str(&json).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn rejects_empty_segments() {
        assert_eq!(err_of("a//b"), FolderPathError::EmptySegment);
        assert_eq!(err_of("/"), FolderPathError::EmptySegment);
        assert_eq!(err_of("/a"), FolderPathError::EmptySegment);
        assert_eq!(err_of("a/"), FolderPathError::EmptySegment);
    }

    #[test]
    fn rejects_dot_segments() {
        assert_eq!(err_of("."), FolderPathError::DotSegment);
        assert_eq!(err_of(".."), FolderPathError::DotSegment);
        assert_eq!(err_of("a/../b"), FolderPathError::DotSegment);
        assert_eq!(err_of("a/./b"), FolderPathError::DotSegment);
    }

    #[test]
    fn rejects_whitespace_only_or_padded_segments() {
        assert_eq!(err_of(" "), FolderPathError::SegmentWhitespace);
        assert_eq!(err_of("a/ b"), FolderPathError::SegmentWhitespace);
        assert_eq!(err_of("a/b "), FolderPathError::SegmentWhitespace);
        assert_eq!(err_of("\ta\t"), FolderPathError::SegmentWhitespace);
    }

    #[test]
    fn rejects_forbidden_characters() {
        // Backslash and C0/C1 controls are forbidden; `/` cannot appear
        // inside a segment because it is the separator.
        assert_eq!(err_of("a\\b"), FolderPathError::ForbiddenCharacter);
        assert_eq!(err_of("a\u{1}b"), FolderPathError::ForbiddenCharacter);
        assert_eq!(err_of("a\u{7f}b"), FolderPathError::ForbiddenCharacter);
        assert_eq!(err_of("a\u{9f}b"), FolderPathError::ForbiddenCharacter);
    }

    #[test]
    fn rejects_oversized_segments_and_paths() {
        let long_segment = "x".repeat(MAX_FOLDER_SEGMENT_BYTES + 1);
        assert_eq!(err_of(&long_segment), FolderPathError::SegmentTooLong);
        // Exactly at the limit is fine.
        let at_limit = "x".repeat(MAX_FOLDER_SEGMENT_BYTES);
        assert!(FolderPath::parse(&at_limit).is_ok());
    }

    #[test]
    fn rejects_too_many_segments() {
        let many = vec!["f"; MAX_FOLDER_SEGMENTS + 1].join("/");
        assert_eq!(err_of(&many), FolderPathError::TooManySegments);
        let exact = vec!["f"; MAX_FOLDER_SEGMENTS].join("/");
        assert!(FolderPath::parse(&exact).is_ok());
    }

    #[test]
    fn rejects_path_longer_than_limit_via_join() {
        // Build a path under the segment caps whose join exceeds the total.
        let seg_len = MAX_FOLDER_SEGMENT_BYTES;
        let seg = "y".repeat(seg_len);
        let count = (MAX_FOLDER_PATH_BYTES / (seg_len + 1)) + 2;
        assert!(count <= MAX_FOLDER_SEGMENTS);
        let joined = vec![seg.as_str(); count].join("/");
        assert!(joined.len() > MAX_FOLDER_PATH_BYTES);
        assert_eq!(err_of(&joined), FolderPathError::PathTooLong);
    }
}
