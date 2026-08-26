/**
 * TypeScript mirror of the live-surface wire contract (issue #18).
 *
 * These types describe EXACTLY what the Rust side serializes for
 * `surface_load` / `surface_invoke` (see
 * `apps/desktop/src-tauri/src/surface.rs`). Closed vocabularies come from
 * serde; unknown names reject on both sides.
 */

import type { WorkspaceSnapshot } from '../studio/types.ts';

/** One interaction gesture (DOMAIN_MODEL.md §4 event vocabulary). */
export type InteractionEvent =
  | 'press'
  | 'release'
  | 'hold_begin'
  | 'hold_end'
  | 'long_press'
  | 'repeat';

export const INTERACTION_EVENTS: readonly InteractionEvent[] = [
  'press',
  'release',
  'hold_begin',
  'hold_end',
  'long_press',
  'repeat',
];

/** Initial-load payload for the live surface. */
export interface SurfaceLoadResult {
  snapshot: WorkspaceSnapshot;
  /** Whether the local Engine composition exists behind this shell. */
  engine_available: boolean;
}

/** Outcome states of one invocation attempt (closed Rust enum mirror). */
export type InvokeStatus = {
  kind: 'refused';
  token: string;
};

/** Authoritative result of one invocation attempt. */
export interface InvokeOutcome {
  control_id: string;
  interaction: string;
  status: InvokeStatus;
}
