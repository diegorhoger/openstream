/**
 * TypeScript mirror of the OpenStream domain v1 wire contract (issue #17).
 *
 * These types describe EXACTLY what the Rust side serializes: field names,
 * casing, and closed vocabularies come from `crates/openstream-domain` serde
 * derives (snake_case tags, deny_unknown_fields). The WebView consumes the
 * same documents Rust validates; structural truth stays in Rust, this file
 * is the typed view of it plus the closed editing-op vocabulary defined by
 * `apps/desktop/src-tauri/src/studio.rs`.
 */

/** Explicit major.minor schema version carried by every document. */
export interface SchemaVersion {
  major: number;
  minor: number;
}

/** Control kinds v1 (additive enum upstream; unknown names reject). */
export type ControlKind = 'button' | 'toggle' | 'page_jump' | 'variable_display';

export const CONTROL_KINDS: readonly ControlKind[] = [
  'button',
  'toggle',
  'page_jump',
  'variable_display',
];

/** Interaction policies v1. */
export type InteractionPolicy = 'press' | 'release' | 'hold' | 'repeat' | 'toggle';

/**
 * Kind→policy admissibility matrix, mirroring `ControlKind::allows` in the
 * domain crate. The variable display is a state sink and admits no policy.
 */
export const KIND_ALLOWS_POLICY: Readonly<
  Record<ControlKind, readonly InteractionPolicy[]>
> = {
  button: ['press', 'release', 'hold', 'repeat', 'toggle'],
  toggle: ['press', 'release', 'toggle'],
  page_jump: ['press'],
  variable_display: [],
};

/** Default interaction policy applied when the user picks none. */
export function defaultPolicyFor(kind: ControlKind): InteractionPolicy | null {
  switch (kind) {
    case 'button':
      return 'press';
    case 'toggle':
      return 'toggle';
    case 'page_jump':
      return 'press';
    case 'variable_display':
      return null;
  }
}

/** Page-relative grid rectangle (cells; origin top-left). */
export interface Geometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** One control surface on a page. */
export interface Control {
  id: string;
  page_id: string;
  kind: ControlKind;
  geometry: Geometry;
  label: string;
  policy: InteractionPolicy | null;
  enabled: boolean;
}

/** Grid dimensions of a page (both axes >= 1). */
export interface GridDimensions {
  columns: number;
  rows: number;
}

/** One page of a deck with its controls. */
export interface Page {
  id: string;
  deck_id: string;
  ordinal: number;
  grid: GridDimensions;
  controls: Control[];
}

/** A deck with its ordered pages and folder-path attribute. */
export interface Deck {
  id: string;
  workspace_id: string;
  title: string;
  revision: number;
  folder_path: string;
  pages: Page[];
  deleted_at: number | null;
}

/** A named, ordered arrangement of deck references. */
export interface Profile {
  id: string;
  workspace_id: string;
  name: string;
  deck_ids: string[];
  /** Explicit switch triggers bound to this profile (issue #19). */
  switch_rules: SwitchRule[];
}

/** One explicit profile-switch trigger (closed Rust enum mirror). */
export type SwitchTrigger =
  | { kind: 'hotkey'; combo: string }
  | { kind: 'app_focus'; app: string };

/** A validated switch rule stored on its target profile. */
export interface SwitchRule {
  id: string;
  profile_id: string;
  workspace_id: string;
  trigger: SwitchTrigger;
  enabled: boolean;
}

/** Versioned envelope around one deck. */
export interface DeckDocument {
  schema_version: SchemaVersion;
  deck: Deck;
}

/** Versioned envelope around one profile. */
export interface ProfileDocument {
  schema_version: SchemaVersion;
  profile: Profile;
}

/** Full snapshot handed to the editor. */
export interface WorkspaceSnapshot {
  decks: DeckDocument[];
  profiles: ProfileDocument[];
}

/** Initial-load payload from the shell bridge. */
export interface LoadResult {
  snapshot: WorkspaceSnapshot;
  autosave_active: boolean;
}

/** Non-blocking warning (grid collisions; reported, never rejected). */
export interface Diagnostic {
  code: string;
  page_id: string;
  control_ids: string[];
}

/** Result of one applied op / undo / redo. */
export interface ApplyOutcome {
  snapshot: WorkspaceSnapshot;
  can_undo: boolean;
  can_redo: boolean;
  /** Whether THIS change was durably autosaved (honesty flag). */
  saved: boolean;
  save_error: string | null;
  diagnostics: Diagnostic[];
}

/** Closed editing vocabulary (serde-tagged `type` on the Rust side). */
export type StudioOp =
  | { type: 'create_deck'; title: string; folder_path: string }
  | { type: 'rename_deck'; deck_id: string; title: string }
  | { type: 'move_deck_to_folder'; deck_id: string; folder_path: string }
  | { type: 'delete_deck'; deck_id: string }
  | { type: 'add_page'; deck_id: string }
  | { type: 'remove_page'; deck_id: string; page_id: string }
  | { type: 'reorder_page'; deck_id: string; page_id: string; to_index: number }
  | { type: 'resize_grid'; deck_id: string; page_id: string; columns: number; rows: number }
  | {
      type: 'add_control';
      page_id: string;
      kind: ControlKind;
      x: number;
      y: number;
      width: number;
      height: number;
      label: string;
      policy: InteractionPolicy | null;
    }
  | { type: 'move_control'; control_id: string; x: number; y: number }
  | { type: 'resize_control'; control_id: string; width: number; height: number }
  | { type: 'set_control_label'; control_id: string; label: string }
  | { type: 'set_control_enabled'; control_id: string; enabled: boolean }
  | { type: 'set_control_kind'; control_id: string; kind: ControlKind }
  | { type: 'set_control_policy'; control_id: string; policy: InteractionPolicy | null }
  | { type: 'remove_control'; control_id: string }
  | { type: 'create_profile'; name: string }
  | { type: 'rename_profile'; profile_id: string; name: string }
  | { type: 'delete_profile'; profile_id: string }
  | { type: 'profile_add_deck'; profile_id: string; deck_id: string }
  | { type: 'profile_remove_deck'; profile_id: string; deck_id: string }
  | { type: 'profile_move_deck'; profile_id: string; deck_id: string; to_index: number }
  | {
      type: 'add_switch_rule';
      profile_id: string;
      trigger_kind: 'hotkey' | 'app_focus';
      trigger_value: string;
    }
  | { type: 'remove_switch_rule'; profile_id: string; rule_id: string }
  | { type: 'set_switch_rule_enabled'; profile_id: string; rule_id: string; enabled: boolean };

/** v1 size ceilings — verbatim mirrors of `limits.rs` in the domain crate. */
export const LIMITS = {
  maxTextBytes: 256,
  maxFolderSegments: 32,
  maxFolderSegmentBytes: 64,
  maxFolderPathBytes: 1024,
  maxPagesPerDeck: 256,
  maxControlsPerPage: 1024,
  maxDecksPerProfile: 128,
  maxSwitchRulesPerProfile: 32,
  maxAppIdentityBytes: 64,
} as const;

/** Supported document version this build reads. */
export const SUPPORTED_SCHEMA_VERSION: SchemaVersion = { major: 1, minor: 0 };
