//! Live deck surface service (issue #18).
//!
//! The authoritative local boundary behind the on-screen control surface:
//! read-only workspace projection plus fail-closed invocation evaluation.
//!
//! Authority boundary this milestone: NO binding vocabulary exists anywhere
//! yet (action configuration was deliberately kept out of #17's op set, and
//! nothing later added it), so there is no graph a surface press could
//! lawfully dispatch. [`evaluate_invocation`] therefore validates every
//! interaction fail-closed against the domain model — identity, existence,
//! enabled flag, state-sink exclusion, and the event/policy admissibility
//! matrix from DOMAIN_MODEL.md §4 — and then refuses BEFORE any admission,
//! journal write, or effect with the typed [`SurfaceError::BindingAbsent`].
//! The refusal is the honest outcome; the WebView renders it as failure
//! state and never as success (SECURITY.md hard rule). When binding
//! authoring lands in a later milestone, this function gains its graph
//! resolution step ahead of Engine admission and every other rule here
//! stays exactly as tested.
//!
//! No OBS consent surface exists here or anywhere else in this milestone
//! (PR #75 binding constraint): source-visibility and input-mute grants are
//! not expressible through any command in this module.

use std::str::FromStr;

use openstream_domain::control::{Control, ControlKind, InteractionPolicy};
use openstream_domain::document::DeckDocument;
use openstream_domain::ids::ControlId;
use serde::{Deserialize, Serialize};

use crate::studio::{CommandError, StudioState, WorkspaceSnapshot};

/// One interaction gesture on a deck key. Closed vocabulary mirroring
/// DOMAIN_MODEL.md §4 events; unknown names reject at deserialization
/// (deny-by-default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionEvent {
    /// Fire on press.
    Press,
    /// Fire on release.
    Release,
    /// Hold window opened (threshold reached while held).
    HoldBegin,
    /// Hold window closed by release.
    HoldEnd,
    /// Long-press threshold reached.
    LongPress,
    /// Repeating fire tick while held.
    Repeat,
}

impl InteractionEvent {
    /// Canonical wire token (also used inside refusal tokens).
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Release => "release",
            Self::HoldBegin => "hold_begin",
            Self::HoldEnd => "hold_end",
            Self::LongPress => "long_press",
            Self::Repeat => "repeat",
        }
    }

    /// Interaction policies whose semantics admit this event
    /// (DOMAIN_MODEL.md §4). Toggle latches on press, so both the plain
    /// press policy and the toggle policy admit press gestures.
    #[must_use]
    pub fn admitting_policies(self) -> &'static [InteractionPolicy] {
        match self {
            Self::Press => &[InteractionPolicy::Press, InteractionPolicy::Toggle],
            Self::Release => &[InteractionPolicy::Release],
            Self::HoldBegin | Self::HoldEnd | Self::LongPress => &[InteractionPolicy::Hold],
            Self::Repeat => &[InteractionPolicy::Repeat],
        }
    }
}

/// Read-only projection for the live surface window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceLoadResult {
    /// Current authored documents (same truth the editor serves).
    pub snapshot: WorkspaceSnapshot,
    /// Whether the local Engine composition exists behind this shell. When
    /// false the surface renders honestly unavailable instead of inviting
    /// interactions that could never be answered.
    pub engine_available: bool,
}

/// Typed failures of one invocation evaluation, mapped to closed-vocabulary
/// tokens shared with the UI catalog. No variant carries user content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    /// The control id string was not a canonical UUIDv7.
    InvalidControlId,
    /// No live control carries that id.
    ControlNotFound,
    /// Disabled controls stay stored but inert.
    ControlDisabled,
    /// Variable displays are state sinks and admit no interaction.
    StateSinkNoInteraction,
    /// The control's interaction policy does not admit this event.
    PolicyMismatch {
        /// Canonical token of the refused event.
        event: &'static str,
    },
    /// Milestone boundary: no action binding vocabulary exists yet, so no
    /// invocation can be admitted. Refused BEFORE any journal write,
    /// admission, or effect — surfaced as failure, never as success.
    BindingAbsent,
}

impl SurfaceError {
    /// Stable token consumed by the UI localization catalog.
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::InvalidControlId => "invalid_id:control".to_owned(),
            Self::ControlNotFound => "not_found:control".to_owned(),
            Self::ControlDisabled => "control_disabled".to_owned(),
            Self::StateSinkNoInteraction => "state_sink_no_interaction".to_owned(),
            Self::PolicyMismatch { event } => format!("policy_mismatch:{event}"),
            Self::BindingAbsent => "binding_absent".to_owned(),
        }
    }
}

impl From<SurfaceError> for CommandError {
    fn from(error: SurfaceError) -> Self {
        Self {
            token: error.token(),
        }
    }
}

/// A control that passed every fail-closed gate ahead of the binding step.
#[derive(Debug)]
pub struct Evaluated<'a> {
    /// The validated control reference.
    pub control: &'a Control,
}

/// Fail-closed evaluation of one interaction against the live decks:
/// canonical id, existence, enabled flag, state-sink exclusion, and the
/// event/policy matrix. Pure; leaves inputs untouched.
///
/// # Errors
/// [`SurfaceError`] for every refusal; nothing is admitted on any path.
pub fn evaluate_invocation<'a>(
    decks: &'a [DeckDocument],
    control_id: &str,
    event: InteractionEvent,
) -> Result<Evaluated<'a>, SurfaceError> {
    let id = ControlId::from_str(control_id).map_err(|_| SurfaceError::InvalidControlId)?;
    for document in decks {
        if document.deck.deleted_at.is_some() {
            continue;
        }
        for page in &document.deck.pages {
            let Some(control) = page.controls.iter().find(|control| control.id == id) else {
                continue;
            };
            if !control.enabled {
                return Err(SurfaceError::ControlDisabled);
            }
            if matches!(control.kind, ControlKind::VariableDisplay) {
                return Err(SurfaceError::StateSinkNoInteraction);
            }
            let Some(policy) = control.policy else {
                // Interactive kinds always carry a policy; a missing one is
                // a policy mismatch by construction.
                return Err(SurfaceError::PolicyMismatch {
                    event: event.token(),
                });
            };
            if !event.admitting_policies().contains(&policy) {
                return Err(SurfaceError::PolicyMismatch {
                    event: event.token(),
                });
            }
            return Ok(Evaluated { control });
        }
    }
    Err(SurfaceError::ControlNotFound)
}

/// Authoritative result of one surface invocation attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvokeOutcome {
    /// Control the gesture targeted (echoed verbatim).
    pub control_id: String,
    /// Canonical token of the evaluated gesture.
    pub interaction: String,
    /// Closed-vocabulary outcome.
    pub status: InvokeStatus,
}

impl InvokeOutcome {
    fn refused(control_id: &str, event: InteractionEvent, error: &SurfaceError) -> Self {
        Self {
            control_id: control_id.to_owned(),
            interaction: event.token().to_owned(),
            status: InvokeStatus::Refused {
                token: error.token(),
            },
        }
    }
}

/// Outcome states of one invocation. This milestone can only refuse
/// pre-admission; admitted/terminal variants arrive together with binding
/// vocabulary and are additive enum members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InvokeStatus {
    /// Fail-closed refusal before any admission, journal write, or effect.
    Refused {
        /// Closed-vocabulary reason token for the localization catalog.
        token: String,
    },
}

/// Loads the read-only surface projection.
///
/// # Errors
/// [`CommandError`] with token "studio-unavailable" when no session could
/// be composed (no data directory).
#[tauri::command]
pub fn surface_load(
    state: tauri::State<'_, StudioState>,
) -> Result<SurfaceLoadResult, CommandError> {
    let snapshot = state.snapshot().ok_or(CommandError {
        token: "studio-unavailable".to_owned(),
    })?;
    Ok(SurfaceLoadResult {
        snapshot,
        engine_available: state.is_available(),
    })
}

/// Evaluates one deck-key gesture fail-closed. This milestone refuses every
/// admissible invocation at the binding gate BEFORE any durable effect;
/// the typed outcome is what the surface renders.
///
/// # Errors
/// [`CommandError`] mapping the typed [`SurfaceError`] refusal tokens.
#[tauri::command]
pub fn surface_invoke(
    state: tauri::State<'_, StudioState>,
    control_id: String,
    interaction: InteractionEvent,
) -> Result<InvokeOutcome, CommandError> {
    let Some(snapshot) = state.snapshot() else {
        return Err(CommandError {
            token: "studio-unavailable".to_owned(),
        });
    };
    let outcome = match evaluate_invocation(&snapshot.decks, &control_id, interaction) {
        // Binding gate: the honest milestone boundary. Nothing has been
        // written, admitted, or dispatched anywhere along this path. The
        // echoed id is the CANONICAL form of the validated control.
        Ok(evaluated) => InvokeOutcome::refused(
            &evaluated.control.id.to_string(),
            interaction,
            &SurfaceError::BindingAbsent,
        ),
        Err(error) => InvokeOutcome::refused(&control_id, interaction, &error),
    };
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{
        InteractionEvent, InvokeOutcome, InvokeStatus, SurfaceError, SurfaceLoadResult,
        evaluate_invocation,
    };
    use crate::studio::{StudioOp, WorkspaceState, apply_op};
    use openstream_domain::ids::WorkspaceId;
    use std::str::FromStr as _;

    fn workspace() -> WorkspaceId {
        WorkspaceId::from_str("018f6a1c-7b21-7000-9f31-000000000000").unwrap()
    }

    /// Deterministic fixture: one deck, one page, four controls covering
    /// every kind and policy combination under test.
    fn fixture() -> WorkspaceState {
        use openstream_domain::control::{ControlKind, InteractionPolicy};
        let mut state = WorkspaceState::default();
        state = apply_op(
            &state,
            &StudioOp::CreateDeck {
                title: "Live".into(),
                folder_path: String::new(),
            },
            workspace(),
        )
        .expect("deck creates");
        let deck_id = state.decks[0].deck.id.to_string();
        state = apply_op(&state, &StudioOp::AddPage { deck_id }, workspace()).expect("page adds");
        let page_id = state.decks[0].deck.pages[0].id.to_string();
        let placements = [
            (
                ControlKind::Button,
                InteractionPolicy::Press,
                "Mute mic",
                0u16,
            ),
            (ControlKind::Button, InteractionPolicy::Hold, "Siren", 2u16),
            (
                ControlKind::Toggle,
                InteractionPolicy::Toggle,
                "Camera",
                4u16,
            ),
            (
                ControlKind::VariableDisplay,
                InteractionPolicy::Press,
                "Viewers",
                6u16,
            ),
        ];
        for (kind, policy, label, x) in placements {
            state = apply_op(
                &state,
                &StudioOp::AddControl {
                    page_id: page_id.clone(),
                    kind,
                    x,
                    y: 0,
                    width: 2,
                    height: 1,
                    label: label.to_owned(),
                    policy: if matches!(kind, ControlKind::VariableDisplay) {
                        None
                    } else {
                        Some(policy)
                    },
                },
                workspace(),
            )
            .expect("control adds");
        }
        state
    }

    fn first_page_controls(state: &WorkspaceState) -> Vec<(String, String)> {
        let deck = &state.decks[0].deck;
        let page_id = deck.pages[0].id.to_string();
        deck.pages[0]
            .controls
            .iter()
            .map(|control| (control.id.to_string(), page_id.clone()))
            .collect()
    }

    #[test]
    fn press_policy_admits_press_and_refuses_other_events() {
        let state = fixture();
        let (mute_id, _) = &first_page_controls(&state)[0];
        let decks = &state.decks;

        assert!(evaluate_invocation(decks, mute_id, InteractionEvent::Press).is_ok());
        for event in [
            InteractionEvent::Release,
            InteractionEvent::HoldBegin,
            InteractionEvent::HoldEnd,
            InteractionEvent::LongPress,
            InteractionEvent::Repeat,
        ] {
            assert_eq!(
                evaluate_invocation(decks, mute_id, event).err(),
                Some(SurfaceError::PolicyMismatch {
                    event: event.token()
                }),
                "{event:?} must not fire a press-policy control"
            );
        }
    }

    #[test]
    fn hold_policy_admits_hold_window_events_and_repeat_refuses() {
        let state = fixture();
        let (siren_id, _) = &first_page_controls(&state)[1];
        let decks = &state.decks;

        for event in [
            InteractionEvent::HoldBegin,
            InteractionEvent::HoldEnd,
            InteractionEvent::LongPress,
        ] {
            assert!(
                evaluate_invocation(decks, siren_id, event).is_ok(),
                "{event:?}"
            );
        }
        assert_eq!(
            evaluate_invocation(decks, siren_id, InteractionEvent::Repeat).err(),
            Some(SurfaceError::PolicyMismatch { event: "repeat" })
        );
    }

    #[test]
    fn toggle_kind_admits_press_gesture_through_toggle_policy() {
        let state = fixture();
        let (camera_id, _) = &first_page_controls(&state)[2];
        assert!(
            evaluate_invocation(&state.decks, camera_id, InteractionEvent::Press).is_ok(),
            "toggle policy admits press"
        );
        assert_eq!(
            evaluate_invocation(&state.decks, camera_id, InteractionEvent::Release).err(),
            Some(SurfaceError::PolicyMismatch { event: "release" })
        );
    }

    #[test]
    fn disabled_state_sink_unknown_and_malformed_all_fail_closed() {
        let mut state = fixture();
        let ids = first_page_controls(&state);
        let (sink_id, _) = &ids[3];

        assert_eq!(
            evaluate_invocation(&state.decks, sink_id, InteractionEvent::Press).err(),
            Some(SurfaceError::StateSinkNoInteraction)
        );

        // Disable the mute button: inert but still stored.
        let (mute_id, _) = ids[0].clone();
        let op = StudioOp::SetControlEnabled {
            control_id: mute_id.clone(),
            enabled: false,
        };
        state = apply_op(&state, &op, workspace()).unwrap();
        assert_eq!(
            evaluate_invocation(&state.decks, &mute_id, InteractionEvent::Press).err(),
            Some(SurfaceError::ControlDisabled)
        );

        assert_eq!(
            evaluate_invocation(
                &state.decks,
                "018f6a1c-7b21-7003-9f31-ffffffffffff",
                InteractionEvent::Press
            )
            .err(),
            Some(SurfaceError::ControlNotFound)
        );
        assert_eq!(
            evaluate_invocation(&state.decks, "not-a-uuid", InteractionEvent::Press).err(),
            Some(SurfaceError::InvalidControlId)
        );
    }

    #[test]
    fn every_admissible_invocation_refuses_at_the_binding_gate() {
        let state = fixture();
        let ids = first_page_controls(&state);
        let cases = [
            (&ids[0].0, InteractionEvent::Press),
            (&ids[1].0, InteractionEvent::HoldBegin),
            (&ids[1].0, InteractionEvent::HoldEnd),
            (&ids[2].0, InteractionEvent::Press),
        ];
        for (control_id, event) in cases {
            evaluate_invocation(&state.decks, control_id, event)
                .expect("admissible before the gate");
            // The gate itself: no graph resolution exists this milestone.
            let outcome = InvokeOutcome::refused(control_id, event, &SurfaceError::BindingAbsent);
            assert!(
                matches!(outcome.status, InvokeStatus::Refused { .. }),
                "{event:?} must never read as success"
            );
            assert_eq!(
                outcome.status,
                InvokeStatus::Refused {
                    token: "binding_absent".to_owned()
                }
            );
        }
    }

    #[test]
    fn outcome_serializes_closed_vocabulary_shape() {
        let state = fixture();
        let (mute_id, _) = first_page_controls(&state)[0].clone();
        let outcome = InvokeOutcome::refused(
            &mute_id,
            InteractionEvent::Press,
            &SurfaceError::BindingAbsent,
        );
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(
            json.contains(r#""status":{"kind":"refused","token":"binding_absent"}"#),
            "unexpected shape: {json}"
        );

        let load = SurfaceLoadResult {
            snapshot: crate::studio::WorkspaceSnapshot::from(&state),
            engine_available: true,
        };
        let json = serde_json::to_string(&load).unwrap();
        assert!(json.contains(r#""engine_available":true"#), "{json}");

        // Unknown event names reject at deserialization (deny-by-default).
        let parsed: Result<InteractionEvent, _> = serde_json::from_str("\"detonate\"");
        assert!(parsed.is_err());
        let parsed: InteractionEvent = serde_json::from_str("\"hold_begin\"").unwrap();
        assert_eq!(parsed.token(), "hold_begin");
    }
}
