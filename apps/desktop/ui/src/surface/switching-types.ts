/**
 * TypeScript mirror of the switching wire contract (issue #19).
 *
 * Describes EXACTLY what the Rust side serializes for `switch_state_load`
 * / `switch_consent` (see `apps/desktop/src-tauri/src/switching.rs`).
 * Closed vocabularies come from serde; unknown names reject on both sides.
 */

/** Typed state of one switching mechanism (hotkeys or app focus). */
export interface MechanismState {
  /** An explicit, unrevoked user grant covers this mechanism right now. */
  granted: boolean;
  /** This build ships a working backend on this platform. */
  supported: boolean;
  /** Closed-vocabulary degradation tokens, sorted, empty when healthy. */
  issues: string[];
}

/** Serializable typed surface state for the whole switching engine. */
export interface SwitchSurfaceState {
  active_profile: string | null;
  hotkeys: MechanismState;
  app_focus: MechanismState;
  rule_count: number;
  board_conflict: boolean;
}

/** Initial-load payload for the switching panel. */
export interface SwitchLoadResult {
  state: SwitchSurfaceState;
}

/** Closed consent-action vocabulary (mirrors Rust `ConsentAction`). */
export type ConsentAction =
  | 'grant_hotkey'
  | 'revoke_hotkey'
  | 'grant_app_focus'
  | 'revoke_app_focus';

export const CONSENT_ACTIONS: readonly ConsentAction[] = [
  'grant_hotkey',
  'revoke_hotkey',
  'grant_app_focus',
  'revoke_app_focus',
];

/**
 * Classifies one degradation token into its message-catalog parts.
 * Tokens look like `<class>:<detail>`; unknown classes still render
 * honestly through their raw text (visible, never silent).
 */
export function parseIssueToken(
  issue: string,
): { kind: string; detail: string } {
  const separator = issue.indexOf(':');
  if (separator < 0) {
    return { kind: issue, detail: '' };
  }
  return { kind: issue.slice(0, separator), detail: issue.slice(separator + 1) };
}
