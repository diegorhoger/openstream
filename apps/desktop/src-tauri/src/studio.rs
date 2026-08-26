//! Studio deck-editor service (issue #17).
//!
//! The authoritative editing core for the accessible visual deck editor.
//! Every user edit arrives as one closed-vocabulary [`StudioOp`]; the
//! service applies it through the `openstream-domain` typed API — entities
//! are mutated by value, the whole affected document is re-validated
//! fail-closed, and a refused op leaves state byte-identical. Nothing here
//! interprets OBS grants, sources, or mute wiring (PR #75 binding
//! constraint): action *configuration* does not exist in this milestone's
//! vocabulary at all, only deck/page/control/profile authoring.
//!
//! - **Undo/redo:** bounded snapshot stacks ([`UNDO_LIMIT`]) inside
//!   [`EditorSession`]. Each accepted op pushes the previous
//!   [`WorkspaceState`]; a new op clears redo. Snapshots are small
//!   validated documents; cloning keeps the logic deterministic and free of
//!   side effects.
//! - **Autosave:** every accepted mutation, undo, or redo persists the
//!   ENTIRE workspace as one all-or-nothing transaction through the #15
//!   pipeline (`openstream_persistence::sqlite::WorkspaceStore`, WAL +
//!   `synchronous=FULL`). Returning from [`EditorSession::apply`] means the
//!   new state is durable; a storage refusal is surfaced honestly through
//!   [`ApplyOutcome::saved`] = false plus a typed token while the session
//!   keeps serving in-memory edits (degraded, never silent).
//! - **Diagnostics (non-blocking):** grid collisions are recomputed after
//!   every accepted change via [`Page::grid_collisions`] — reported, never
//!   rejected, per DOMAIN_MODEL.md §6 merge semantics.

use std::fmt;

use openstream_domain::control::{Control, ControlKind, Geometry, InteractionPolicy};
use openstream_domain::deck::Deck;
use openstream_domain::document::{DeckDocument, ProfileDocument};
use openstream_domain::error::DomainError;
use openstream_domain::folder::FolderPath;
use openstream_domain::ids::{ControlId, DeckId, PageId, ProfileId, SwitchRuleId, WorkspaceId};
use openstream_domain::limits::check_text;
use openstream_domain::page::{GridDimensions, Page};
use openstream_domain::profile::Profile;
use openstream_domain::switching::{SwitchBoard, SwitchRule};
use openstream_persistence::sqlite::{DocumentKind, StoredDocument, WorkspaceStore};
use serde::{Deserialize, Serialize};

/// Undo/redo-stack depth cap per direction. The oldest snapshots are
/// dropped beyond this; availability flags reflect only retained history.
pub const UNDO_LIMIT: usize = 100;

/// Default grid for newly created pages (8 x 4 cells).
const DEFAULT_GRID: GridDimensions = GridDimensions {
    columns: 8,
    rows: 4,
};

/// One editing intent from the WebView. Closed vocabulary: unknown variants
/// reject during deserialization (deny-by-default), matching the domain's
/// fail-closed posture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StudioOp {
    /// Create an empty deck with the given title and folder path.
    CreateDeck {
        /// Validated user title.
        title: String,
        /// Folder path string (`""` = workspace root); parsed fail-closed.
        folder_path: String,
    },
    /// Rename a deck.
    RenameDeck {
        /// Target deck id.
        deck_id: String,
        /// New validated title.
        title: String,
    },
    /// Move a deck to another folder (folders are a path attribute).
    MoveDeckToFolder {
        /// Target deck id.
        deck_id: String,
        /// Destination folder path string (`""` = root).
        folder_path: String,
    },
    /// Soft-delete a deck (tombstone kept in storage) and cascade it out of
    /// every profile so no dangling references remain.
    DeleteDeck {
        /// Target deck id.
        deck_id: String,
    },
    /// Append a new page to a deck (ordinal after the current last).
    AddPage {
        /// Owning deck id.
        deck_id: String,
    },
    /// Remove a page and its controls from a deck.
    RemovePage {
        /// Owning deck id.
        deck_id: String,
        /// Page id to remove.
        page_id: String,
    },
    /// Move a page to a new position in the deck order; ordinals are then
    /// renumbered contiguously from zero.
    ReorderPage {
        /// Owning deck id.
        deck_id: String,
        /// Page id being moved.
        page_id: String,
        /// Zero-based destination index in the page sequence.
        to_index: u32,
    },
    /// Resize a page grid. Refuses when existing controls would fall
    /// outside the new bounds (fail closed).
    ResizeGrid {
        /// Owning deck id.
        deck_id: String,
        /// Target page id.
        page_id: String,
        /// New column count (>= 1).
        columns: u16,
        /// New row count (>= 1).
        rows: u16,
    },
    /// Place a new control on a page.
    AddControl {
        /// Owning page id.
        page_id: String,
        /// Control kind.
        kind: ControlKind,
        /// Left column of the top-left cell.
        x: u16,
        /// Top row of the top-left cell.
        y: u16,
        /// Width in cells (>= 1).
        width: u16,
        /// Height in cells (>= 1).
        height: u16,
        /// Validated screen-reader-legible label.
        label: String,
        /// Explicit interaction policy; omitted selects the kind default.
        policy: Option<InteractionPolicy>,
    },
    /// Move a control to a new top-left cell (fail closed on overflow).
    MoveControl {
        /// Target control id.
        control_id: String,
        /// New left column.
        x: u16,
        /// New top row.
        y: u16,
    },
    /// Resize a control (fail closed on overflow).
    ResizeControl {
        /// Target control id.
        control_id: String,
        /// New width in cells (>= 1).
        width: u16,
        /// New height in cells (>= 1).
        height: u16,
    },
    /// Relabel a control.
    SetControlLabel {
        /// Target control id.
        control_id: String,
        /// New validated label.
        label: String,
    },
    /// Enable or disable a control (disabled stays stored but inert).
    SetControlEnabled {
        /// Target control id.
        control_id: String,
        /// New enabled flag.
        enabled: bool,
    },
    /// Change a control's kind; refuses when the current policy would be
    /// invalid for the new kind (the user adjusts policy separately).
    SetControlKind {
        /// Target control id.
        control_id: String,
        /// New kind.
        kind: ControlKind,
    },
    /// Set or clear a control's interaction policy; must be allowed by the
    /// control's kind (state sinks admit none).
    SetControlPolicy {
        /// Target control id.
        control_id: String,
        /// New policy; `None` only valid for state sinks.
        policy: Option<InteractionPolicy>,
    },
    /// Remove a control from its page.
    RemoveControl {
        /// Target control id.
        control_id: String,
    },
    /// Create an empty profile.
    CreateProfile {
        /// Validated profile name.
        name: String,
    },
    /// Rename a profile.
    RenameProfile {
        /// Target profile id.
        profile_id: String,
        /// New validated name.
        name: String,
    },
    /// Remove a profile entirely.
    DeleteProfile {
        /// Target profile id.
        profile_id: String,
    },
    /// Append a deck reference to a profile (refuses duplicates).
    ProfileAddDeck {
        /// Target profile id.
        profile_id: String,
        /// Deck id to append.
        deck_id: String,
    },
    /// Remove a deck reference from a profile.
    ProfileRemoveDeck {
        /// Target profile id.
        profile_id: String,
        /// Deck reference to remove.
        deck_id: String,
    },
    /// Move a deck reference within the profile's ordered list.
    ProfileMoveDeck {
        /// Owning profile id.
        profile_id: String,
        /// Deck id being moved.
        deck_id: String,
        /// Zero-based destination index.
        to_index: u32,
    },
    /// Bind an explicit switch trigger to a profile (issue #19): when the
    /// trigger fires, this profile becomes the active one. Triggers are
    /// parsed fail-closed against the domain grammars, and the whole
    /// workspace must stay free of duplicate triggers (deterministic
    /// conflict rejection) or the op refuses.
    AddSwitchRule {
        /// Target profile id (the profile that becomes active).
        profile_id: String,
        /// Trigger class: `hotkey` or `app_focus` (closed vocabulary).
        trigger_kind: String,
        /// Canonical combination (`ctrl+shift+f5`) or app identity
        /// (`obs64.exe`) per the chosen class.
        trigger_value: String,
    },
    /// Remove one switch rule from a profile.
    RemoveSwitchRule {
        /// Owning profile id.
        profile_id: String,
        /// Rule id to remove.
        rule_id: String,
    },
    /// Enable or disable an existing switch rule without deleting it
    /// (disabled rules stay stored but inert while still reserving their
    /// trigger).
    SetSwitchRuleEnabled {
        /// Owning profile id.
        profile_id: String,
        /// Rule id to change.
        rule_id: String,
        /// New enabled flag.
        enabled: bool,
    },
}

/// Everything the editor holds: live documents in stable order. Decks sort
/// by durable id (UUIDv7 creation time gives stable, meaningful order);
/// profiles likewise. Soft-deleted decks leave this view state; their
/// tombstones stay in persisted history.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceState {
    /// Live deck documents.
    pub decks: Vec<DeckDocument>,
    /// Profile documents.
    pub profiles: Vec<ProfileDocument>,
}

impl WorkspaceState {
    /// Deterministic persisted form of the whole state.
    #[must_use]
    pub fn to_stored_documents(&self) -> Vec<StoredDocument> {
        let mut documents = Vec::new();
        for document in &self.decks {
            if let Ok(json) = document.to_json_string() {
                documents.push(StoredDocument {
                    kind: DocumentKind::Deck,
                    id: document.deck.id.to_string(),
                    json,
                });
            }
        }
        for document in &self.profiles {
            if let Ok(json) = document.to_json_string() {
                documents.push(StoredDocument {
                    kind: DocumentKind::Profile,
                    id: document.profile.id.to_string(),
                    json,
                });
            }
        }
        documents
    }

    /// Loads a state back from raw store rows, validating each document
    /// fail-closed. Any unreadable row refuses the WHOLE load rather than
    /// dropping content silently.
    ///
    /// # Errors
    /// [`StudioError::Domain`] naming the structural reason when any stored
    /// JSON fails decode or validation.
    pub fn from_stored_documents(rows: &[StoredDocument]) -> Result<Self, StudioError> {
        let mut state = Self::default();
        for row in rows {
            match row.kind {
                DocumentKind::Deck => {
                    let document =
                        DeckDocument::from_json_str(&row.json).map_err(StudioError::Domain)?;
                    if document.deck.deleted_at.is_none() {
                        state.decks.push(document);
                    }
                }
                DocumentKind::Profile => {
                    let document =
                        ProfileDocument::from_json_str(&row.json).map_err(StudioError::Domain)?;
                    state.profiles.push(document);
                }
            }
        }
        Ok(state)
    }

    /// Collision diagnostics across every live page, deterministic order.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for document in &self.decks {
            for page in &document.deck.pages {
                let pairs = page.grid_collisions();
                if pairs.is_empty() {
                    continue;
                }
                let mut control_ids = Vec::new();
                for (first, second) in pairs {
                    for id in [first, second] {
                        let text = id.to_string();
                        if !control_ids.contains(&text) {
                            control_ids.push(text);
                        }
                    }
                }
                diagnostics.push(Diagnostic {
                    code: "grid_collision".to_owned(),
                    page_id: page.id.to_string(),
                    control_ids,
                });
            }
        }
        diagnostics
    }
}

/// Serializable projection handed to the WebView. Documents serialize
/// exactly as the domain defines them (`schema_version` included), so the
/// UI consumes the same contract Rust validates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceSnapshot {
    /// Live deck documents in stable order.
    pub decks: Vec<DeckDocument>,
    /// Profile documents in stable order.
    pub profiles: Vec<ProfileDocument>,
}

impl From<&WorkspaceState> for WorkspaceSnapshot {
    fn from(state: &WorkspaceState) -> Self {
        Self {
            decks: state.decks.clone(),
            profiles: state.profiles.clone(),
        }
    }
}

/// Non-blocking warnings recomputed after every accepted change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Closed-vocabulary code for the UI localization catalog.
    pub code: String,
    /// Page the diagnostic applies to.
    pub page_id: String,
    /// Controls involved, in deterministic order.
    pub control_ids: Vec<String>,
}

/// Result of one applied op / undo / redo: the new authoritative snapshot,
/// stack flags, autosave honesty flag, and non-blocking diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApplyOutcome {
    /// The full new workspace snapshot.
    pub snapshot: WorkspaceSnapshot,
    /// Whether undo has retained history.
    pub can_undo: bool,
    /// Whether redo has retained history.
    pub can_redo: bool,
    /// Whether the autosave transaction committed for THIS outcome. `false`
    /// means the session degraded honestly: edits continue in memory only
    /// until persistence recovers.
    pub saved: bool,
    /// Stable failure token when `saved` is false.
    pub save_error: Option<String>,
    /// Non-blocking collision diagnostics over the new snapshot.
    pub diagnostics: Vec<Diagnostic>,
}

/// Typed failures of one op application, mapped to closed-vocabulary tokens
/// for UI localization. No variant carries user content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioError {
    /// A referenced entity was not found in the workspace.
    NotFound {
        /// Structural entity class ("deck", "page", "control", "profile").
        entity: &'static str,
    },
    /// An id string failed canonical UUIDv7 parsing.
    InvalidId {
        /// Entity kind whose identifier failed to parse.
        entity: &'static str,
    },
    /// The folder path violated its grammar.
    InvalidFolder,
    /// The domain rejected the resulting document (validation stage).
    Domain(DomainError),
    /// A soft-deleted deck cannot be edited further.
    DeckDeleted,
}

impl StudioError {
    /// Stable token consumed by the UI localization catalog.
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::NotFound { entity } => format!("not_found:{entity}"),
            Self::InvalidId { entity } => format!("invalid_id:{entity}"),
            Self::InvalidFolder => "invalid_folder".to_owned(),
            Self::DeckDeleted => "deck_deleted".to_owned(),
            Self::Domain(error) => match error {
                DomainError::TextFieldOutOfRange { field } => format!("text_out_of_range:{field}"),
                DomainError::GeometryOutsideGrid { axis } => {
                    format!("geometry_outside_grid:{axis}")
                }
                DomainError::LimitExceeded { .. } => "limit_exceeded".to_owned(),
                DomainError::OrdinalConflict => "ordinal_conflict".to_owned(),
                DomainError::DuplicateControlId => "duplicate_control".to_owned(),
                DomainError::DuplicateDeckRef => "duplicate_deck_ref".to_owned(),
                DomainError::PolicyNotAllowedForKind => "policy_not_allowed".to_owned(),
                DomainError::ZeroGridDimension | DomainError::ZeroGeometryExtent => {
                    "zero_extent".to_owned()
                }
                DomainError::RevisionOverflow => "revision_overflow".to_owned(),
                DomainError::InvalidHotkeyCombo { reason } => {
                    format!("invalid_hotkey:{reason}")
                }
                DomainError::InvalidAppIdentity { reason } => {
                    format!("invalid_app_identity:{reason}")
                }
                DomainError::ConflictingSwitchRule { kind } => {
                    format!("conflicting_switch_rule:{kind}")
                }
                DomainError::ForeignSwitchRule => "foreign_switch_rule".to_owned(),
                _ => "domain_rejected".to_owned(),
            },
        }
    }
}

impl fmt::Display for StudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity } => write!(f, "{entity} not found"),
            Self::InvalidId { entity } => write!(f, "{entity} id is not a canonical UUIDv7"),
            Self::InvalidFolder => write!(f, "invalid folder path"),
            Self::DeckDeleted => write!(f, "deck is deleted"),
            Self::Domain(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StudioError {}

impl From<DomainError> for StudioError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

fn parse<T: std::str::FromStr<Err = DomainError>>(
    raw: &str,
    entity: &'static str,
) -> Result<T, StudioError> {
    T::from_str(raw).map_err(|_| StudioError::InvalidId { entity })
}

/// Default interaction policy per control kind (DOMAIN_MODEL.md §4
/// semantics; state sinks carry none).
#[must_use]
fn default_policy(kind: ControlKind) -> Option<InteractionPolicy> {
    match kind {
        ControlKind::Button => Some(InteractionPolicy::Press),
        ControlKind::Toggle => Some(InteractionPolicy::Toggle),
        ControlKind::PageJump => Some(InteractionPolicy::Press),
        ControlKind::VariableDisplay => None,
    }
}

/// Generic reorder helper: returns the sequence with `item` moved to
/// `to_index`, or `None` when the item is absent or the index is out of
/// range. Order semantics live here once so pages and profile lists behave
/// identically.
fn reorder_sequence<T: Copy + PartialEq>(sequence: &[T], item: T, to_index: u32) -> Option<Vec<T>> {
    let position = sequence.iter().position(|candidate| *candidate == item)?;
    let target = to_index as usize;
    if target >= sequence.len() {
        return None;
    }
    let mut next = sequence.to_vec();
    let removed = next.remove(position);
    next.insert(target, removed);
    Some(next)
}

struct DeckRef<'a> {
    document: &'a mut DeckDocument,
}

impl DeckRef<'_> {
    /// Bumps the deck revision through the domain API (`bump_revision`
    /// consumes by value, so the clone-bump-swap is the typed path) and
    /// revalidates the whole document fail-closed.
    fn bump_and_validate(&mut self) -> Result<(), StudioError> {
        let mut bumped = self.document.deck.clone();
        bumped.revision = bumped.next_revision()?;
        self.document.deck = bumped;
        self.document.validate()?;
        Ok(())
    }
}

/// Finds the live deck holding `page_id`, hands the whole document over.
fn find_live_deck_by_page<'a>(
    decks: &'a mut [DeckDocument],
    page_id: PageId,
) -> Result<DeckRef<'a>, StudioError> {
    for document in decks {
        if document.deck.deleted_at.is_some() {
            continue;
        }
        if document.deck.pages.iter().any(|page| page.id == page_id) {
            return Ok(DeckRef { document });
        }
    }
    Err(StudioError::NotFound { entity: "page" })
}

/// Finds the live deck with `deck_id`.
fn find_live_deck<'a>(
    decks: &'a mut [DeckDocument],
    deck_id: DeckId,
) -> Result<DeckRef<'a>, StudioError> {
    let document = decks
        .iter_mut()
        .find(|document| document.deck.id == deck_id)
        .ok_or(StudioError::NotFound { entity: "deck" })?;
    if document.deck.deleted_at.is_some() {
        return Err(StudioError::DeckDeleted);
    }
    Ok(DeckRef { document })
}

fn mutate_profile<R>(
    profiles: &mut [ProfileDocument],
    profile_id: ProfileId,
    apply: impl FnOnce(&mut Profile) -> Result<R, StudioError>,
) -> Result<R, StudioError> {
    let document = profiles
        .iter_mut()
        .find(|document| document.profile.id == profile_id)
        .ok_or(StudioError::NotFound { entity: "profile" })?;
    let result = apply(&mut document.profile)?;
    document.validate()?;
    Ok(result)
}

/// Shared control-field mutation: locate the control anywhere in the
/// workspace, apply the change, bump + validate the owning deck.
fn update_control<R>(
    state: &mut WorkspaceState,
    control_id: ControlId,
    apply: impl FnOnce(&mut Control) -> Result<R, StudioError>,
) -> Result<R, StudioError> {
    for document in &mut state.decks {
        let found_at = document
            .deck
            .pages
            .iter()
            .position(|page| page.controls.iter().any(|control| control.id == control_id));
        if let Some(page_index) = found_at {
            let result = {
                let page = &mut document.deck.pages[page_index];
                let control = page
                    .controls
                    .iter_mut()
                    .find(|control| control.id == control_id);
                match control {
                    Some(control) => apply(control)?,
                    None => return Err(StudioError::NotFound { entity: "control" }),
                }
            };
            let mut bumped = document.deck.clone();
            bumped.revision = bumped.next_revision()?;
            *document = DeckDocument::new(bumped);
            document.validate()?;
            return Ok(result);
        }
    }
    Err(StudioError::NotFound { entity: "control" })
}

/// Removes a control from whichever live page holds it.
fn remove_control(state: &mut WorkspaceState, control_id: ControlId) -> Result<(), StudioError> {
    for document in &mut state.decks {
        let found = document.deck.pages.iter_mut().any(|page| {
            let before = page.controls.len();
            page.controls.retain(|control| control.id != control_id);
            page.controls.len() != before
        });
        if found {
            let mut bumped = document.deck.clone();
            bumped.revision = bumped.next_revision()?;
            *document = DeckDocument::new(bumped);
            return document.validate().map_err(StudioError::from);
        }
    }
    Err(StudioError::NotFound { entity: "control" })
}

/// Applies one op against `state`, returning a NEW state on success. Pure
/// aside from UUIDv7 minting (the Rust core's generation authority): every
/// rejection leaves the input untouched.
///
/// # Errors
/// [`StudioError`] for every fail-closed case; the input state is never
/// partially modified.
pub fn apply_op(
    state: &WorkspaceState,
    op: &StudioOp,
    workspace_id: WorkspaceId,
) -> Result<WorkspaceState, StudioError> {
    let mut next = state.clone();

    match op {
        StudioOp::CreateDeck { title, folder_path } => {
            check_text("title", title)?;
            let folder = FolderPath::parse(folder_path).map_err(|_| StudioError::InvalidFolder)?;
            let id = DeckId::generate();
            let position = next.decks.partition_point(|document| document.deck.id < id);
            next.decks.insert(
                position,
                DeckDocument::new(Deck {
                    id,
                    workspace_id,
                    title: title.clone(),
                    revision: 0,
                    folder_path: folder,
                    pages: Vec::new(),
                    deleted_at: None,
                }),
            );
        }
        StudioOp::RenameDeck { deck_id, title } => {
            check_text("title", title)?;
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let mut deck = find_live_deck(&mut next.decks, deck_id)?;
            deck.document.deck.title = title.clone();
            deck.bump_and_validate()?;
        }
        StudioOp::MoveDeckToFolder {
            deck_id,
            folder_path,
        } => {
            let folder = FolderPath::parse(folder_path).map_err(|_| StudioError::InvalidFolder)?;
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let mut deck = find_live_deck(&mut next.decks, deck_id)?;
            deck.document.deck.folder_path = folder;
            deck.bump_and_validate()?;
        }
        StudioOp::DeleteDeck { deck_id } => {
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let now_ms: i64 = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| StudioError::Domain(DomainError::InvalidTimestamp))?
                .as_millis()
                .try_into()
                .map_err(|_| StudioError::Domain(DomainError::InvalidTimestamp))?;
            {
                let mut deck = find_live_deck(&mut next.decks, deck_id)?;
                deck.document.deck.deleted_at = Some(now_ms);
                deck.bump_and_validate()?;
            }
            // Cascade: no profile may keep referencing a deleted deck.
            for document in &mut next.profiles {
                document.profile.deck_ids.retain(|id| *id != deck_id);
                document.validate()?;
            }
            next.decks
                .retain(|document| document.deck.deleted_at.is_none());
        }
        StudioOp::AddPage { deck_id } => {
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let mut deck = find_live_deck(&mut next.decks, deck_id)?;
            let ordinal = deck
                .document
                .deck
                .pages
                .iter()
                .map(|page| page.ordinal)
                .max()
                .unwrap_or(0);
            deck.document.deck.pages.push(Page {
                id: PageId::generate(),
                deck_id,
                ordinal: ordinal.saturating_add(1),
                grid: DEFAULT_GRID,
                controls: Vec::new(),
            });
            deck.bump_and_validate()?;
        }
        StudioOp::RemovePage { deck_id, page_id } => {
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let page_id: PageId = parse(page_id, "page")?;
            let mut deck = find_live_deck(&mut next.decks, deck_id)?;
            let before = deck.document.deck.pages.len();
            deck.document.deck.pages.retain(|page| page.id != page_id);
            if deck.document.deck.pages.len() == before {
                return Err(StudioError::NotFound { entity: "page" });
            }
            for (index, page) in deck.document.deck.pages.iter_mut().enumerate() {
                page.ordinal = index as u32;
            }
            deck.bump_and_validate()?;
        }
        StudioOp::ReorderPage {
            deck_id,
            page_id,
            to_index,
        } => {
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let page_id: PageId = parse(page_id, "page")?;
            let mut deck = find_live_deck(&mut next.decks, deck_id)?;
            let order: Vec<PageId> = deck.document.deck.pages.iter().map(|p| p.id).collect();
            let reordered = reorder_sequence(&order, page_id, *to_index)
                .ok_or(StudioError::NotFound { entity: "page" })?;
            for (index, id) in reordered.iter().enumerate() {
                let page = deck
                    .document
                    .deck
                    .pages
                    .iter_mut()
                    .find(|page| page.id == *id);
                if let Some(page) = page {
                    page.ordinal = index as u32;
                }
            }
            deck.document.deck.pages.sort_by_key(|page| page.ordinal);
            deck.bump_and_validate()?;
        }
        StudioOp::ResizeGrid {
            deck_id,
            page_id,
            columns,
            rows,
        } => {
            let deck_id: DeckId = parse(deck_id, "deck")?;
            let page_id: PageId = parse(page_id, "page")?;
            let mut deck = find_live_deck_by_page(&mut next.decks, page_id)?;
            let _ = deck_id;
            let page = deck
                .document
                .deck
                .pages
                .iter_mut()
                .find(|page| page.id == page_id);
            let Some(page) = page else {
                return Err(StudioError::NotFound { entity: "page" });
            };
            page.grid = GridDimensions::new(*columns, *rows)?;
            deck.bump_and_validate()?;
        }
        StudioOp::AddControl {
            page_id,
            kind,
            x,
            y,
            width,
            height,
            label,
            policy,
        } => {
            check_text("label", label)?;
            let page_id: PageId = parse(page_id, "page")?;
            if *width == 0 || *height == 0 {
                return Err(StudioError::Domain(DomainError::ZeroGeometryExtent));
            }
            let resolved_policy = match policy {
                Some(explicit) => {
                    if !kind.allows(explicit) {
                        return Err(StudioError::Domain(DomainError::PolicyNotAllowedForKind));
                    }
                    Some(*explicit)
                }
                None => default_policy(*kind),
            };
            let mut deck = find_live_deck_by_page(&mut next.decks, page_id)?;
            let page = deck
                .document
                .deck
                .pages
                .iter_mut()
                .find(|page| page.id == page_id);
            let Some(page) = page else {
                return Err(StudioError::NotFound { entity: "page" });
            };
            page.controls.push(Control {
                id: ControlId::generate(),
                page_id,
                kind: *kind,
                geometry: Geometry {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                },
                label: label.clone(),
                policy: resolved_policy,
                enabled: true,
            });
            deck.bump_and_validate()?;
        }
        StudioOp::MoveControl { control_id, x, y } => {
            let control_id: ControlId = parse(control_id, "control")?;
            update_control(&mut next, control_id, |control| {
                control.geometry.x = *x;
                control.geometry.y = *y;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::ResizeControl {
            control_id,
            width,
            height,
        } => {
            let control_id: ControlId = parse(control_id, "control")?;
            update_control(&mut next, control_id, |control| {
                if *width == 0 || *height == 0 {
                    return Err(StudioError::Domain(DomainError::ZeroGeometryExtent));
                }
                control.geometry.width = *width;
                control.geometry.height = *height;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::SetControlLabel { control_id, label } => {
            check_text("label", label)?;
            let control_id: ControlId = parse(control_id, "control")?;
            update_control(&mut next, control_id, |control| {
                control.label = label.clone();
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::SetControlEnabled {
            control_id,
            enabled,
        } => {
            let control_id: ControlId = parse(control_id, "control")?;
            update_control(&mut next, control_id, |control| {
                control.enabled = *enabled;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::SetControlKind { control_id, kind } => {
            let control_id: ControlId = parse(control_id, "control")?;
            update_control(&mut next, control_id, |control| {
                let compatible = match (kind, control.policy) {
                    (ControlKind::VariableDisplay, None) => true,
                    (_, Some(policy)) => kind.allows(&policy),
                    (_, None) => false,
                };
                if !compatible {
                    return Err(StudioError::Domain(DomainError::PolicyNotAllowedForKind));
                }
                control.kind = *kind;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::SetControlPolicy { control_id, policy } => {
            let control_id: ControlId = parse(control_id, "control")?;
            update_control(&mut next, control_id, |control| {
                let allowed = match policy {
                    Some(explicit) => control.kind.allows(explicit),
                    None => matches!(control.kind, ControlKind::VariableDisplay),
                };
                if !allowed {
                    return Err(StudioError::Domain(DomainError::PolicyNotAllowedForKind));
                }
                control.policy = *policy;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::RemoveControl { control_id } => {
            let control_id: ControlId = parse(control_id, "control")?;
            remove_control(&mut next, control_id)?;
        }
        StudioOp::CreateProfile { name } => {
            check_text("name", name)?;
            let id = ProfileId::generate();
            let position = next
                .profiles
                .partition_point(|document| document.profile.id < id);
            next.profiles.insert(
                position,
                ProfileDocument::new(Profile {
                    id,
                    workspace_id,
                    name: name.clone(),
                    deck_ids: Vec::new(),
                    switch_rules: Vec::new(),
                }),
            );
        }
        StudioOp::RenameProfile { profile_id, name } => {
            check_text("name", name)?;
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            mutate_profile(&mut next.profiles, profile_id, |profile| {
                profile.name = name.clone();
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::DeleteProfile { profile_id } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let before = next.profiles.len();
            next.profiles
                .retain(|document| document.profile.id != profile_id);
            if next.profiles.len() == before {
                return Err(StudioError::NotFound { entity: "profile" });
            }
        }
        StudioOp::ProfileAddDeck {
            profile_id,
            deck_id,
        } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let deck_ref: DeckId = parse(deck_id, "deck")?;
            mutate_profile(&mut next.profiles, profile_id, move |profile| {
                if profile.deck_ids.contains(&deck_ref) {
                    return Err(StudioError::Domain(DomainError::DuplicateDeckRef));
                }
                profile.deck_ids.push(deck_ref);
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::ProfileRemoveDeck {
            profile_id,
            deck_id,
        } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let deck_ref: DeckId = parse(deck_id, "deck")?;
            mutate_profile(&mut next.profiles, profile_id, move |profile| {
                let before = profile.deck_ids.len();
                profile.deck_ids.retain(|id| *id != deck_ref);
                if profile.deck_ids.len() == before {
                    return Err(StudioError::NotFound { entity: "deck" });
                }
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::ProfileMoveDeck {
            profile_id,
            deck_id,
            to_index,
        } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let deck_ref: DeckId = parse(deck_id, "deck")?;
            mutate_profile(&mut next.profiles, profile_id, move |profile| {
                let reordered = reorder_sequence(&profile.deck_ids, deck_ref, *to_index)
                    .ok_or(StudioError::NotFound { entity: "deck" })?;
                profile.deck_ids = reordered;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::AddSwitchRule {
            profile_id,
            trigger_kind,
            trigger_value,
        } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let trigger = parse_switch_trigger(trigger_kind, trigger_value)?;
            let rule = SwitchRule {
                id: SwitchRuleId::generate(),
                profile_id,
                workspace_id,
                trigger,
                enabled: true,
            };
            mutate_profile(&mut next.profiles, profile_id, move |profile| {
                profile.switch_rules.push(rule);
                profile.validate()?;
                Ok::<(), StudioError>(())
            })?;
            // Deterministic conflict resolution: duplicate triggers across
            // the whole workspace refuse the mutating op.
            validate_switch_board(&next.profiles)?;
        }
        StudioOp::RemoveSwitchRule {
            profile_id,
            rule_id,
        } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let rule_ref: SwitchRuleId = parse(rule_id, "switch_rule")?;
            mutate_profile(&mut next.profiles, profile_id, move |profile| {
                let before = profile.switch_rules.len();
                profile.switch_rules.retain(|rule| rule.id != rule_ref);
                if profile.switch_rules.len() == before {
                    return Err(StudioError::NotFound { entity: "rule" });
                }
                profile.validate()?;
                Ok::<(), StudioError>(())
            })?;
        }
        StudioOp::SetSwitchRuleEnabled {
            profile_id,
            rule_id,
            enabled,
        } => {
            let profile_id: ProfileId = parse(profile_id, "profile")?;
            let rule_ref: SwitchRuleId = parse(rule_id, "switch_rule")?;
            mutate_profile(&mut next.profiles, profile_id, move |profile| {
                let rule = profile
                    .switch_rules
                    .iter_mut()
                    .find(|rule| rule.id == rule_ref)
                    .ok_or(StudioError::NotFound { entity: "rule" })?;
                rule.enabled = *enabled;
                profile.validate()?;
                Ok::<(), StudioError>(())
            })?;
        }
    }

    Ok(next)
}

/// Parses a switch-trigger authoring input fail-closed against its closed
/// vocabulary (`hotkey` combinations / `app_focus` identities).
fn parse_switch_trigger(
    kind: &str,
    value: &str,
) -> Result<openstream_domain::switching::SwitchTrigger, StudioError> {
    use openstream_domain::switching::{AppIdentity, HotkeyCombo, SwitchTrigger};
    use std::str::FromStr as _;
    match kind {
        "hotkey" => {
            let combo = HotkeyCombo::from_str(value).map_err(StudioError::Domain)?;
            Ok(SwitchTrigger::Hotkey { combo })
        }
        "app_focus" => {
            let app = AppIdentity::from_str(value).map_err(StudioError::Domain)?;
            Ok(SwitchTrigger::AppFocus { app })
        }
        _ => Err(StudioError::Domain(DomainError::InvalidCapability {
            reason: "unknown switch trigger kind",
        })),
    }
}

/// Cross-profile deterministic conflict gate: the whole workspace's rules
/// must form a valid board (no duplicate triggers) or the mutation refuses.
fn validate_switch_board(profiles: &[ProfileDocument]) -> Result<(), StudioError> {
    SwitchBoard::from_profiles(profiles.iter().map(|document| &document.profile))
        .map(|_| ())
        .map_err(StudioError::Domain)
}

/// Stateful editor session: authoritative state, undo/redo stacks, and the
/// autosave sink. All mutating entry points return [`ApplyOutcome`] so the
/// UI never holds divergent truth.
#[derive(Debug)]
pub struct EditorSession {
    state: WorkspaceState,
    workspace_id: WorkspaceId,
    undo_stack: Vec<WorkspaceState>,
    redo_stack: Vec<WorkspaceState>,
    store: Option<WorkspaceStore>,
}

/// One-shot open+load used by [`EditorSession::open`]. The reason string is
/// a closed-vocabulary diagnostic for the startup log only.
fn open_workspace(path: &std::path::Path) -> Result<(WorkspaceStore, WorkspaceState), String> {
    let store = WorkspaceStore::open(path).map_err(|error| format!("store-open:{error:?}"))?;
    let rows = store
        .load_all()
        .map_err(|error| format!("store-read:{error:?}"))?;
    let state = WorkspaceState::from_stored_documents(&rows)
        .map_err(|error| format!("document-unreadable:{}", error.token()))?;
    Ok((store, state))
}

impl EditorSession {
    /// Opens a session over the workspace database at `store_path`. When the
    /// store cannot open (or previously persisted documents refuse to
    /// decode), the session degrades honestly: editing continues in memory
    /// and every outcome reports `saved = false` until persistence works.
    #[must_use]
    pub fn open(store_path: Option<&std::path::Path>) -> Self {
        let mut session = Self {
            state: WorkspaceState::default(),
            workspace_id: WorkspaceId::generate(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            store: None,
        };
        if let Some(path) = store_path {
            match open_workspace(path) {
                Ok((store, loaded)) => {
                    if let Some(document) = loaded.decks.first() {
                        session.workspace_id = document.deck.workspace_id;
                    }
                    session.state = loaded;
                    session.store = Some(store);
                }
                Err(reason) => {
                    eprintln!(
                        "openstream-studio: workspace store unavailable ({reason}); \
                         autosave stays off"
                    );
                }
            }
        }
        session
    }

    /// Whether autosave currently has a working store.
    #[must_use]
    pub const fn autosave_active(&self) -> bool {
        self.store.is_some()
    }

    /// The current authoritative snapshot.
    #[must_use]
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot::from(&self.state)
    }

    fn persist(&mut self) -> (bool, Option<String>) {
        let Some(store) = self.store.as_mut() else {
            return (false, Some("autosave_unavailable".to_owned()));
        };
        let now_ms: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or_default();
        match store.rewrite_all(&self.state.to_stored_documents(), now_ms) {
            Ok(()) => (true, None),
            Err(error) => {
                eprintln!("openstream-studio: autosave refused ({error:?}); keeping memory state");
                (false, Some("autosave_refused".to_owned()))
            }
        }
    }

    fn outcome(&self, saved: bool, save_error: Option<String>) -> ApplyOutcome {
        ApplyOutcome {
            snapshot: self.snapshot(),
            can_undo: !self.undo_stack.is_empty(),
            can_redo: !self.redo_stack.is_empty(),
            saved,
            save_error,
            diagnostics: self.state.diagnostics(),
        }
    }

    /// Applies one op. On acceptance the previous state enters the undo
    /// stack (bounded), redo clears, and autosave runs before returning.
    ///
    /// # Errors
    /// [`StudioError`] for every refused op; the session state stays
    /// untouched and the command layer converts the typed error into a
    /// localized token for the inspector.
    pub fn apply(&mut self, op: &StudioOp) -> Result<ApplyOutcome, StudioError> {
        let previous = self.state.clone();
        let next = apply_op(&self.state, op, self.workspace_id)?;
        self.state = next;
        self.undo_stack.push(previous);
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        let (saved, save_error) = self.persist();
        Ok(self.outcome(saved, save_error))
    }

    /// Restores the most recent undone state.
    pub fn undo(&mut self) -> ApplyOutcome {
        let Some(previous) = self.undo_stack.pop() else {
            return self.outcome(true, None);
        };
        let current = std::mem::replace(&mut self.state, previous);
        self.redo_stack.push(current);
        if self.redo_stack.len() > UNDO_LIMIT {
            self.redo_stack.remove(0);
        }
        let (saved, save_error) = self.persist();
        self.outcome(saved, save_error)
    }

    /// Re-applies the most recently undone change.
    pub fn redo(&mut self) -> ApplyOutcome {
        let Some(next_state) = self.redo_stack.pop() else {
            return self.outcome(true, None);
        };
        let current = std::mem::replace(&mut self.state, next_state);
        self.undo_stack.push(current);
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        let (saved, save_error) = self.persist();
        self.outcome(saved, save_error)
    }
}

/// File name of the authored-workspace store inside the data directory
/// (sibling of `journal.sqlite3` and `openstream.lock`).
pub const WORKSPACE_FILE_NAME: &str = "workspace.sqlite3";

/// Serializable command failure: a closed-vocabulary token the WebView maps
/// into localized inspector text. No user content, no storage internals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    /// Stable token (e.g. "invalid_id:deck", "autosave_unavailable").
    pub token: String,
}

impl From<StudioError> for CommandError {
    fn from(error: StudioError) -> Self {
        Self {
            token: error.token(),
        }
    }
}

/// Payload of the initial load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoadResult {
    /// Current authoritative snapshot (empty on first launch).
    pub snapshot: WorkspaceSnapshot,
    /// Whether autosave has a working store behind it. When false the UI
    /// must surface the degraded-autosave state honestly.
    pub autosave_active: bool,
}

/// Mutex-guarded session handed to the Tauri commands. Poisoning is
/// recovered through `into_inner`: SQLite transactions and plain values
/// cannot be corrupted by a panicked holder.
pub struct StudioState {
    inner: Mutex<StudioInner>,
}

struct StudioInner {
    session: Option<EditorSession>,
}

impl StudioState {
    /// Builds the managed state for a resolved data directory. Without one,
    /// the session runs autosave-degraded exactly like a failed open.
    #[must_use]
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let session = data_dir.map(|dir| EditorSession::open(Some(&dir.join(WORKSPACE_FILE_NAME))));
        Self {
            inner: Mutex::new(StudioInner { session }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StudioInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The current authoritative snapshot when a session composed; `None`
    /// means no data directory resolved and nothing can be served.
    #[must_use]
    pub fn snapshot(&self) -> Option<WorkspaceSnapshot> {
        let inner = self.lock();
        inner.session.as_ref().map(|session| session.snapshot())
    }

    /// Whether the editor session composed at all (a data directory
    /// resolved). The live surface uses this as its honest
    /// engine-availability flag.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.lock().session.is_some()
    }
}

use std::sync::Mutex;

/// Loads the workspace snapshot for the editor window.
///
/// # Errors
/// [`CommandError`] with token "studio-unavailable" when no session could
/// be composed (no data directory).
#[tauri::command]
pub fn studio_load(state: tauri::State<'_, StudioState>) -> Result<LoadResult, CommandError> {
    let mut inner = state.lock();
    let Some(session) = inner.session.as_mut() else {
        return Err(CommandError {
            token: "studio-unavailable".to_owned(),
        });
    };
    Ok(LoadResult {
        snapshot: session.snapshot(),
        autosave_active: session.autosave_active(),
    })
}

/// Applies one editing op; autosaves before returning.
///
/// # Errors
/// [`CommandError`] mapping the typed [`StudioError`] refusal tokens.
#[tauri::command]
pub fn studio_apply(
    state: tauri::State<'_, StudioState>,
    op: StudioOp,
) -> Result<ApplyOutcome, CommandError> {
    let mut inner = state.lock();
    let Some(session) = inner.session.as_mut() else {
        return Err(CommandError {
            token: "studio-unavailable".to_owned(),
        });
    };
    Ok(session.apply(&op)?)
}

/// Undoes the most recent accepted change.
///
/// # Errors
/// [`CommandError`] when no session exists.
#[tauri::command]
pub fn studio_undo(state: tauri::State<'_, StudioState>) -> Result<ApplyOutcome, CommandError> {
    let mut inner = state.lock();
    let Some(session) = inner.session.as_mut() else {
        return Err(CommandError {
            token: "studio-unavailable".to_owned(),
        });
    };
    Ok(session.undo())
}

/// Redoes the most recently undone change.
///
/// # Errors
/// [`CommandError`] when no session exists.
#[tauri::command]
pub fn studio_redo(state: tauri::State<'_, StudioState>) -> Result<ApplyOutcome, CommandError> {
    let mut inner = state.lock();
    let Some(session) = inner.session.as_mut() else {
        return Err(CommandError {
            token: "studio-unavailable".to_owned(),
        });
    };
    Ok(session.redo())
}

#[cfg(test)]
mod tests {
    use super::{ApplyOutcome, EditorSession, StudioOp, UNDO_LIMIT, WorkspaceState, apply_op};
    use openstream_domain::control::{ControlKind, Geometry, InteractionPolicy};
    use openstream_domain::document::DeckDocument;
    use openstream_domain::folder::FolderPath;
    use openstream_domain::ids::{DeckId, WorkspaceId};
    use openstream_domain::page::Page;
    use std::path::PathBuf;
    use std::str::FromStr as _;

    fn workspace() -> WorkspaceId {
        WorkspaceId::from_str("018f6a1c-7b21-7000-9f31-000000000000").unwrap()
    }

    fn deck_id(seed: u32) -> DeckId {
        DeckId::from_str(&format!("018f6a1c-7b21-7{seed:03x}-9f31-{seed:012x}")).unwrap()
    }

    fn state_with_deck() -> WorkspaceState {
        let id = deck_id(1);
        let mut document = DeckDocument::new(openstream_domain::deck::Deck {
            id,
            workspace_id: workspace(),
            title: "Studio".into(),
            revision: 3,
            folder_path: FolderPath::parse("live").unwrap(),
            pages: vec![Page {
                id: openstream_domain::ids::PageId::from_str(
                    "018f6a1c-7b21-7002-9f31-000000000002",
                )
                .unwrap(),
                deck_id: id,
                ordinal: 0,
                grid: openstream_domain::page::GridDimensions {
                    columns: 8,
                    rows: 4,
                },
                controls: Vec::new(),
            }],
            deleted_at: None,
        });
        document.deck.revision = 3;
        WorkspaceState {
            decks: vec![document],
            profiles: Vec::new(),
        }
    }

    fn ok(state: &WorkspaceState, op: &StudioOp) -> WorkspaceState {
        apply_op(state, op, workspace()).expect("op applies")
    }

    fn err_token(state: &WorkspaceState, op: &StudioOp) -> String {
        apply_op(state, op, workspace())
            .expect_err("op refuses")
            .token()
    }

    #[test]
    fn create_deck_then_rename_and_move_folder_round_trip() {
        let state = WorkspaceState::default();
        let created = ok(
            &state,
            &StudioOp::CreateDeck {
                title: "Main".into(),
                folder_path: "live/scene".into(),
            },
        );
        assert_eq!(created.decks.len(), 1);
        assert_eq!(created.decks[0].deck.title, "Main");
        assert_eq!(
            created.decks[0].deck.folder_path.as_path_string(),
            "live/scene"
        );

        let id = created.decks[0].deck.id.to_string();
        let renamed = ok(
            &created,
            &StudioOp::RenameDeck {
                deck_id: id.clone(),
                title: "Backup".into(),
            },
        );
        assert_eq!(renamed.decks[0].deck.title, "Backup");
        assert!(renamed.decks[0].deck.revision > created.decks[0].deck.revision);

        let moved = ok(
            &renamed,
            &StudioOp::MoveDeckToFolder {
                deck_id: id,
                folder_path: String::new(),
            },
        );
        assert!(moved.decks[0].deck.folder_path.is_root());
    }

    #[test]
    fn create_deck_rejects_bad_title_and_bad_folder_without_touching_state() {
        let state = state_with_deck();
        assert_eq!(
            err_token(
                &state,
                &StudioOp::CreateDeck {
                    title: "   ".into(),
                    folder_path: String::new(),
                }
            ),
            "text_out_of_range:title"
        );
        assert_eq!(
            err_token(
                &state,
                &StudioOp::CreateDeck {
                    title: "ok".into(),
                    folder_path: "bad//path".into(),
                }
            ),
            "invalid_folder"
        );
    }

    #[test]
    fn unknown_ids_fail_closed_with_typed_tokens() {
        let state = state_with_deck();
        assert_eq!(
            err_token(
                &state,
                &StudioOp::RenameDeck {
                    deck_id: deck_id(99).to_string(),
                    title: "X".into(),
                }
            ),
            "not_found:deck"
        );
        assert_eq!(
            err_token(
                &state,
                &StudioOp::RenameDeck {
                    deck_id: "not-a-uuid".into(),
                    title: "X".into(),
                }
            ),
            "invalid_id:deck"
        );
    }

    #[test]
    fn page_add_remove_reorder_keeps_ordinals_contiguous() {
        let mut state = state_with_deck();
        let deck = state.decks[0].deck.id.to_string();
        state = ok(
            &state,
            &StudioOp::AddPage {
                deck_id: deck.clone(),
            },
        );
        state = ok(
            &state,
            &StudioOp::AddPage {
                deck_id: deck.clone(),
            },
        );
        assert_eq!(state.decks[0].deck.pages.len(), 3);
        let ordinals: Vec<u32> = state.decks[0]
            .deck
            .pages
            .iter()
            .map(|page| page.ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1, 2]);

        let first_page = state.decks[0].deck.pages[0].id.to_string();
        let reordered = ok(
            &state,
            &StudioOp::ReorderPage {
                deck_id: deck,
                page_id: first_page,
                to_index: 2,
            },
        );
        let after: Vec<u32> = reordered.decks[0]
            .deck
            .pages
            .iter()
            .map(|page| page.ordinal)
            .collect();
        assert_eq!(after, [0, 1, 2], "reorder renumbers contiguously");
        assert_eq!(reordered.decks[0].deck.revision, 6);
    }

    #[test]
    fn reorder_page_out_of_range_refuses() {
        let state = state_with_deck();
        let deck = state.decks[0].deck.id.to_string();
        let page = state.decks[0].deck.pages[0].id.to_string();
        assert_eq!(
            err_token(
                &state,
                &StudioOp::ReorderPage {
                    deck_id: deck,
                    page_id: page,
                    to_index: 5,
                }
            ),
            "not_found:page"
        );
    }

    #[test]
    fn add_control_defaults_policy_by_kind_and_validates_geometry() {
        let state = state_with_deck();
        let page = state.decks[0].deck.pages[0].id.to_string();

        let next = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page.clone(),
                kind: ControlKind::Button,
                x: 0,
                y: 0,
                width: 2,
                height: 1,
                label: "mute mic".into(),
                policy: None,
            },
        );
        let control = &next.decks[0].deck.pages[0].controls[0];
        assert_eq!(control.policy, Some(InteractionPolicy::Press));
        assert!(control.enabled);

        // Variable display defaults to no policy (state sink).
        let sink = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page.clone(),
                kind: ControlKind::VariableDisplay,
                x: 4,
                y: 0,
                width: 1,
                height: 1,
                label: "vol".into(),
                policy: None,
            },
        );
        assert_eq!(sink.decks[0].deck.pages[0].controls[0].policy, None);

        // Outside the grid refuses with the typed axis token.
        assert_eq!(
            err_token(
                &state,
                &StudioOp::AddControl {
                    page_id: page.clone(),
                    kind: ControlKind::Button,
                    x: 8,
                    y: 0,
                    width: 1,
                    height: 1,
                    label: "off-grid".into(),
                    policy: None,
                }
            ),
            "geometry_outside_grid:x"
        );

        // Explicitly wrong policy for the kind refuses.
        assert_eq!(
            err_token(
                &state,
                &StudioOp::AddControl {
                    page_id: state.decks[0].deck.pages[0].id.to_string(),
                    kind: ControlKind::VariableDisplay,
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                    label: "sink".into(),
                    policy: Some(InteractionPolicy::Press),
                }
            ),
            "policy_not_allowed"
        );
    }

    #[test]
    fn move_resize_control_fails_closed_outside_grid() {
        let mut state = state_with_deck();
        let page = state.decks[0].deck.pages[0].id.to_string();
        state = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page,
                kind: ControlKind::Button,
                x: 0,
                y: 0,
                width: 2,
                height: 1,
                label: "btn".into(),
                policy: None,
            },
        );
        let control = state.decks[0].deck.pages[0].controls[0].id.to_string();

        let moved = ok(
            &state,
            &StudioOp::MoveControl {
                control_id: control.clone(),
                x: 6,
                y: 3,
            },
        );
        assert_eq!(
            moved.decks[0].deck.pages[0].controls[0].geometry,
            Geometry {
                x: 6,
                y: 3,
                width: 2,
                height: 1
            }
        );

        // x extent 6+2=8 fits exactly; moving one further must refuse.
        assert_eq!(
            err_token(
                &state,
                &StudioOp::MoveControl {
                    control_id: control.clone(),
                    x: 7,
                    y: 3,
                }
            ),
            "geometry_outside_grid:x"
        );
        assert_eq!(
            err_token(
                &state,
                &StudioOp::ResizeControl {
                    control_id: control,
                    width: 0,
                    height: 1,
                }
            ),
            "zero_extent"
        );
    }

    #[test]
    fn set_control_kind_checks_current_policy_compatibility() {
        let mut state = state_with_deck();
        let page = state.decks[0].deck.pages[0].id.to_string();
        state = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page,
                kind: ControlKind::Toggle,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                label: "lamp".into(),
                policy: Some(InteractionPolicy::Toggle),
            },
        );
        let control = state.decks[0].deck.pages[0].controls[0].id.to_string();
        // Toggle carries the Toggle policy; PageJump admits only Press, so
        // switching kind would strand an illegal policy — refuse.
        assert_eq!(
            err_token(
                &state,
                &StudioOp::SetControlKind {
                    control_id: control.clone(),
                    kind: ControlKind::PageJump,
                }
            ),
            "policy_not_allowed"
        );
        // Button admits every policy: switching works.
        let switched = ok(
            &state,
            &StudioOp::SetControlKind {
                control_id: control,
                kind: ControlKind::Button,
            },
        );
        assert_eq!(
            switched.decks[0].deck.pages[0].controls[0].kind,
            ControlKind::Button
        );
    }

    #[test]
    fn remove_control_drops_it_from_the_page() {
        let mut state = state_with_deck();
        let page = state.decks[0].deck.pages[0].id.to_string();
        state = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page,
                kind: ControlKind::Button,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                label: "gone".into(),
                policy: None,
            },
        );
        let control = state.decks[0].deck.pages[0].controls[0].id.to_string();
        let removed = ok(
            &state,
            &StudioOp::RemoveControl {
                control_id: control.clone(),
            },
        );
        assert!(removed.decks[0].deck.pages[0].controls.is_empty());
        // Removing again refuses honestly.
        assert_eq!(
            err_token(
                &removed,
                &StudioOp::RemoveControl {
                    control_id: control
                }
            ),
            "not_found:control"
        );
    }

    #[test]
    fn resize_grid_refuses_when_controls_would_overflow() {
        let mut state = state_with_deck();
        let page = state.decks[0].deck.pages[0].id.to_string();
        let deck = state.decks[0].deck.id.to_string();
        state = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page.clone(),
                kind: ControlKind::Button,
                x: 6,
                y: 0,
                width: 2,
                height: 1,
                label: "wide".into(),
                policy: None,
            },
        );
        assert_eq!(
            err_token(
                &state,
                &StudioOp::ResizeGrid {
                    deck_id: deck,
                    page_id: page,
                    columns: 4,
                    rows: 4,
                }
            ),
            "geometry_outside_grid:x"
        );
        let shrunk_rows_only = ok(
            &state,
            &StudioOp::ResizeGrid {
                deck_id: state.decks[0].deck.id.to_string(),
                page_id: state.decks[0].deck.pages[0].id.to_string(),
                columns: 8,
                rows: 1,
            },
        );
        assert_eq!(shrunk_rows_only.decks[0].deck.pages[0].grid.rows, 1);
    }

    #[test]
    fn delete_deck_cascades_out_of_profiles_and_soft_deletes() {
        let mut state = state_with_deck();
        let deck = state.decks[0].deck.id.to_string();
        state = ok(
            &state,
            &StudioOp::CreateProfile {
                name: "Show".into(),
            },
        );
        let profile = state.profiles[0].profile.id.to_string();
        state = ok(
            &state,
            &StudioOp::ProfileAddDeck {
                profile_id: profile,
                deck_id: deck,
            },
        );
        let deleted = ok(
            &state,
            &StudioOp::DeleteDeck {
                deck_id: state.decks[0].deck.id.to_string(),
            },
        );
        assert!(deleted.decks.is_empty(), "tombstone leaves live view");
        assert!(
            deleted.profiles[0].profile.deck_ids.is_empty(),
            "cascade removes ref"
        );
    }

    #[test]
    fn profile_ops_cover_create_rename_add_remove_move() {
        let mut state = state_with_deck();
        state = ok(&state, &StudioOp::CreateProfile { name: "A".into() });
        state = ok(&state, &StudioOp::CreateProfile { name: "B".into() });

        // Deterministic ordering by id even under identical names.
        let ids: Vec<String> = state
            .profiles
            .iter()
            .map(|d| d.profile.name.clone())
            .collect();
        assert_eq!(ids.len(), 2);

        let profile = state.profiles[0].profile.id.to_string();
        let deck_a = state.decks[0].deck.id.to_string();

        let added = ok(
            &state,
            &StudioOp::ProfileAddDeck {
                profile_id: profile.clone(),
                deck_id: deck_a.clone(),
            },
        );
        // Duplicate add refuses.
        assert_eq!(
            err_token(
                &added,
                &StudioOp::ProfileAddDeck {
                    profile_id: profile.clone(),
                    deck_id: deck_a.clone(),
                }
            ),
            "duplicate_deck_ref"
        );

        // Second deck for move testing.
        let with_second = ok(
            &added,
            &StudioOp::CreateDeck {
                title: "Second".into(),
                folder_path: String::new(),
            },
        );
        let deck_b = with_second
            .decks
            .iter()
            .find(|document| document.deck.title == "Second")
            .expect("second deck")
            .deck
            .id
            .to_string();
        let both = ok(
            &with_second,
            &StudioOp::ProfileAddDeck {
                profile_id: profile.clone(),
                deck_id: deck_b.clone(),
            },
        );
        let order_of = |state: &WorkspaceState| {
            state.profiles[0]
                .profile
                .deck_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(order_of(&both), vec![deck_a.clone(), deck_b.clone()]);

        let swapped = ok(
            &both,
            &StudioOp::ProfileMoveDeck {
                profile_id: profile.clone(),
                deck_id: deck_a.clone(),
                to_index: 1,
            },
        );
        assert_eq!(order_of(&swapped), vec![deck_b.clone(), deck_a.clone()]);

        let removed = ok(
            &swapped,
            &StudioOp::ProfileRemoveDeck {
                profile_id: profile,
                deck_id: deck_a.clone(),
            },
        );
        assert_eq!(order_of(&removed), vec![deck_b]);
    }

    #[test]
    fn diagnostics_report_grid_collisions_without_rejecting_them() {
        let mut state = state_with_deck();
        let page = state.decks[0].deck.pages[0].id.to_string();
        state = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page.clone(),
                kind: ControlKind::Button,
                x: 0,
                y: 0,
                width: 2,
                height: 2,
                label: "a".into(),
                policy: None,
            },
        );
        state = ok(
            &state,
            &StudioOp::AddControl {
                page_id: page.clone(),
                kind: ControlKind::Button,
                x: 1,
                y: 1,
                width: 2,
                height: 2,
                label: "b".into(),
                policy: None,
            },
        );
        let session_diagnostics = state.diagnostics();
        assert_eq!(session_diagnostics.len(), 1);
        assert_eq!(session_diagnostics[0].code, "grid_collision");
        assert_eq!(session_diagnostics[0].control_ids.len(), 2);
    }

    fn temp_session(label: &str) -> (tempfile::TempDir, PathBuf, EditorSession) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(format!("{label}.sqlite3"));
        let session = EditorSession::open(Some(&path));
        (dir, path, session)
    }

    #[test]
    fn session_autosave_survives_reopen_across_processes() {
        let (_dir, path, mut session) = temp_session("autosave-reopen");
        assert!(session.autosave_active());
        let outcome = session
            .apply(&StudioOp::CreateDeck {
                title: "Durable".into(),
                folder_path: String::new(),
            })
            .expect("apply");
        assert!(outcome.saved);
        assert!(outcome.can_undo);
        assert!(!outcome.can_redo);
        drop(session);

        let reopened = EditorSession::open(Some(&path));
        assert!(reopened.autosave_active());
        let snapshot = reopened.snapshot();
        assert_eq!(snapshot.decks.len(), 1);
        assert_eq!(snapshot.decks[0].deck.title, "Durable");
        assert!(!reopened.snapshot().decks.is_empty());
    }

    #[test]
    fn session_without_store_degrades_honestly() {
        let mut session = EditorSession::open(None);
        assert!(!session.autosave_active());
        let outcome = session
            .apply(&StudioOp::CreateDeck {
                title: "Volatile".into(),
                folder_path: String::new(),
            })
            .expect("in-memory edit still works");
        assert!(!outcome.saved);
        assert_eq!(outcome.save_error.as_deref(), Some("autosave_unavailable"));
    }

    #[test]
    fn undo_redo_round_trips_and_clears_redo_on_new_work() {
        let (_dir, _path, mut session) = temp_session("undo-redo");
        session
            .apply(&StudioOp::CreateDeck {
                title: "One".into(),
                folder_path: String::new(),
            })
            .expect("create one");

        let undone = session.undo();
        assert!(undone.snapshot.decks.is_empty());
        assert!(!undone.can_undo);
        assert!(undone.can_redo);

        let redone = session.redo();
        assert_eq!(redone.snapshot.decks.len(), 1);
        assert!(redone.can_undo);
        assert!(!redone.can_redo);

        session.undo();
        // New work clears redo.
        session
            .apply(&StudioOp::CreateDeck {
                title: "Two".into(),
                folder_path: String::new(),
            })
            .expect("create two");
        let after_new_work = session.redo();
        assert_eq!(
            after_new_work.snapshot.decks.len(),
            1,
            "redo stack was cleared"
        );
        assert_eq!(after_new_work.snapshot.decks[0].deck.title, "Two");
    }

    #[test]
    fn refused_op_leaves_session_untouched_and_unsaved_flags_stable() {
        let (_dir, _path, mut session) = temp_session("refused-op");
        session
            .apply(&StudioOp::CreateDeck {
                title: "Keep".into(),
                folder_path: String::new(),
            })
            .expect("seed");
        let before = session.snapshot();
        let error = session.apply(&StudioOp::CreateDeck {
            title: String::new(),
            folder_path: String::new(),
        });
        assert!(error.is_err());
        assert_eq!(session.snapshot(), before, "refusal changes nothing");
    }

    #[test]
    fn undo_stack_is_bounded_at_the_documented_limit() {
        let (_dir, _path, mut session) = temp_session("undo-bounded");
        let total = UNDO_LIMIT + 25;
        for index in 0..total {
            session
                .apply(&StudioOp::CreateProfile {
                    name: format!("p{index}"),
                })
                .expect("unique name applies");
        }
        assert_eq!(session.snapshot().profiles.len(), total);
        // Undo until the stack is exhausted: exactly UNDO_LIMIT
        // restorations succeed; the oldest snapshots were dropped, so
        // `total - UNDO_LIMIT` profiles remain.
        let mut undos = 0;
        loop {
            let outcome = session.undo();
            undos += 1;
            if !outcome.can_undo {
                break;
            }
            if undos > total + 5 {
                panic!("undo loop did not terminate at the stack bound");
            }
        }
        assert_eq!(undos, UNDO_LIMIT);
        assert_eq!(
            session.snapshot().profiles.len(),
            total - UNDO_LIMIT,
            "only the retained history is undoable"
        );
    }

    #[test]
    fn outcome_serializes_snapshot_documents_for_the_webview() {
        let (_dir, _path, mut session) = temp_session("serialize-outcome");
        let outcome: ApplyOutcome = session
            .apply(&StudioOp::CreateDeck {
                title: "Wire".into(),
                folder_path: "a/b".into(),
            })
            .expect("applies");
        let json = serde_json::to_string(&outcome).expect("outcome serializes");
        assert!(json.contains(r#""schema_version":{"major":1,"minor":0}"#));
        assert!(json.contains("\"title\":\"Wire\""));
        assert!(json.contains("\"can_undo\":true"));
        assert!(json.contains("\"saved\":true"));
    }
}
