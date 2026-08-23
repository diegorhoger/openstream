//! Controls: kinds, interaction policies, visual states, and geometry
//! (DOMAIN_MODEL.md §4).

use crate::ids::{ControlId, PageId};
use serde::{Deserialize, Serialize};

/// Control kinds v1 (additive enum; forward-compatible additions are a domain
/// minor change, DOMAIN_MODEL.md §4). Unknown variant names reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    /// Momentary action surface.
    Button,
    /// Latched action surface.
    Toggle,
    /// Navigate to a target page.
    PageJump,
    /// Render one variable value (state sink; no interactions).
    VariableDisplay,
}

impl ControlKind {
    /// Whether this kind admits `policy` as its interaction policy. The
    /// variable display is a state sink and admits no policy at all.
    #[must_use]
    pub const fn allows(&self, policy: &InteractionPolicy) -> bool {
        match self {
            Self::Button => true,
            // Toggle latches on press; hold/repeat semantics are button-only.
            Self::Toggle => matches!(
                policy,
                InteractionPolicy::Press | InteractionPolicy::Release | InteractionPolicy::Toggle
            ),
            Self::PageJump => matches!(policy, InteractionPolicy::Press),
            Self::VariableDisplay => false,
        }
    }
}

/// Interaction policies v1, derived from PRD Stage 1 semantics. Timeout,
/// cancellation, and fail-fast live inside the action graph, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionPolicy {
    /// Fire on press.
    Press,
    /// Fire on release.
    Release,
    /// Hold-to-fire window.
    Hold,
    /// Repeating fire while held.
    Repeat,
    /// Latched toggle semantics.
    Toggle,
}

/// Visual control states v1: typed and exhaustive (PRD Stage 1 must-ship).
/// Derived from Engine journal evidence only — never claimed optimistically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualState {
    /// No activity.
    Idle,
    /// Physically/logically pressed.
    Pressed,
    /// Armed destructive binding awaiting confirmation.
    Armed,
    /// A bound graph is running.
    Running,
    /// Last run succeeded (authoritative Engine result).
    Success,
    /// Last run failed (authoritative Engine result).
    Failure,
    /// Disabled by the user or validation.
    Disabled,
    /// Owning engine/peer unreachable.
    Disconnected,
}

/// Page-relative grid rectangle of a control. Origin is the top-left cell;
/// extents are in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    /// Left column (0-based).
    pub x: u16,
    /// Top row (0-based).
    pub y: u16,
    /// Width in cells; at least one.
    pub width: u16,
    /// Height in cells; at least one.
    pub height: u16,
}

/// One control surface on a page.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// Durable identifier (UUIDv7).
    pub id: ControlId,
    /// Owning page.
    pub page_id: PageId,
    /// Kind.
    pub kind: ControlKind,
    /// Grid geometry.
    pub geometry: Geometry,
    /// User label; screen-reader-legible name. Never logged or serialized
    /// into evidence (taxonomy redaction rules).
    pub label: String,
    /// Interaction policy; `None` only for state sinks.
    pub policy: Option<InteractionPolicy>,
    /// Enabled flag; disabled controls stay stored but inert.
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::{ControlKind, Geometry, InteractionPolicy, VisualState};
    use crate::ids::{ControlId, PageId};
    use std::str::FromStr as _;

    fn id7(seed: usize) -> String {
        let hex = "0123456789abcdef";
        format!(
            "018f6a1c-7b21-7cc0-9f3{}-0e3d5a9d4c11",
            &hex[seed % 16..seed % 16 + 1]
        )
    }

    #[test]
    fn kind_policy_matrix() {
        let policies = [
            InteractionPolicy::Press,
            InteractionPolicy::Release,
            InteractionPolicy::Hold,
            InteractionPolicy::Repeat,
            InteractionPolicy::Toggle,
        ];
        // Button admits every v1 policy.
        for p in &policies {
            assert!(ControlKind::Button.allows(p), "{p:?} on button");
        }
        // Toggle latches: press, release, and toggle semantics only.
        assert!(ControlKind::Toggle.allows(&InteractionPolicy::Press));
        assert!(ControlKind::Toggle.allows(&InteractionPolicy::Release));
        assert!(ControlKind::Toggle.allows(&InteractionPolicy::Toggle));
        assert!(!ControlKind::Toggle.allows(&InteractionPolicy::Hold));
        assert!(!ControlKind::Toggle.allows(&InteractionPolicy::Repeat));
        // Page jump fires on press only.
        assert!(ControlKind::PageJump.allows(&InteractionPolicy::Press));
        for p in &policies[1..] {
            assert!(!ControlKind::PageJump.allows(p), "{p:?} on page_jump");
        }
    }

    #[test]
    fn enums_serialize_snake_case_and_exhaustively() {
        assert_eq!(
            serde_json::to_string(&ControlKind::PageJump).unwrap(),
            "\"page_jump\""
        );
        assert_eq!(
            serde_json::to_string(&ControlKind::VariableDisplay).unwrap(),
            "\"variable_display\""
        );
        assert_eq!(
            serde_json::to_string(&VisualState::Disconnected).unwrap(),
            "\"disconnected\""
        );
        let back: VisualState = serde_json::from_str("\"armed\"").unwrap();
        assert_eq!(back, VisualState::Armed);
    }

    #[test]
    fn geometry_serializes_field_order_deterministically() {
        let g = Geometry {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(json, r#"{"x":1,"y":2,"width":3,"height":4}"#);
    }

    #[test]
    fn control_ids_parse_independently_of_page_ids() {
        let control = ControlId::from_str(&id7(1)).unwrap();
        let page = PageId::from_str(&id7(2)).unwrap();
        assert_ne!(control.as_uuid(), page.as_uuid());
    }
}
