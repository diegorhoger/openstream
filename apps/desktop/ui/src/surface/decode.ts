/**
 * Fail-closed client-side decoding of surface load results (issue #18).
 *
 * Mirrors the validation invariants of the Rust `surface_load` command so
 * the live surface refuses malformed projections before rendering anything.
 * The Rust service remains the AUTHORITATIVE gate; a disagreement resolves
 * toward refusal, never toward acceptance.
 */

import { validateDeckDocument, validateProfileDocument } from '../studio/decode.ts';
import type { WorkspaceSnapshot } from '../studio/types.ts';
import { INTERACTION_EVENTS } from './types.ts';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function asSnapshot(value: unknown): WorkspaceSnapshot | null {
  if (!isRecord(value) || !Array.isArray(value.decks) || !Array.isArray(value.profiles)) {
    return null;
  }
  return value as unknown as WorkspaceSnapshot;
}

/**
 * Validates one load result. Returns closed-vocabulary error tokens; an
 * empty list means the projection is safe to render.
 */
export function surfaceLoadErrors(result: unknown): string[] {
  if (!isRecord(result)) {
    return ['surface_load_invalid'];
  }
  const errors: string[] = [];
  const typed = asSnapshot(result.snapshot);
  if (typed === null) {
    return ['surface_load_invalid'];
  }
  for (const deck of typed.decks) {
    errors.push(...validateDeckDocument(deck));
  }
  for (const profile of typed.profiles) {
    errors.push(...validateProfileDocument(profile));
  }
  if (typeof result.engine_available !== 'boolean') {
    errors.push('surface_engine_flag_invalid');
  }
  return [...new Set(errors)];
}

/** Canonical interaction-event tokens accepted across the bridge. */
export function isInteractionEvent(value: string): value is (typeof INTERACTION_EVENTS)[number] {
  return (INTERACTION_EVENTS as readonly string[]).includes(value);
}

/**
 * Validates one invocation outcome. Refusals are EXPECTED honest outcomes,
 * not decode failures — only structural violations refuse here.
 */
export function invokeOutcomeErrors(outcome: unknown): string[] {
  if (!isRecord(outcome)) {
    return ['invoke_outcome_invalid'];
  }
  const errors: string[] = [];
  if (typeof outcome.control_id !== 'string' || outcome.control_id.length === 0) {
    errors.push('invoke_outcome_invalid');
  }
  if (
    typeof outcome.interaction !== 'string' ||
    !isInteractionEvent(outcome.interaction)
  ) {
    errors.push('invoke_interaction_unknown');
  }
  const status = outcome.status;
  if (!isRecord(status)) {
    errors.push('invoke_outcome_invalid');
  } else if (status.kind === 'refused') {
    if (typeof status.token !== 'string' || status.token.length === 0) {
      errors.push('invoke_outcome_invalid');
    }
  } else {
    errors.push('invoke_status_unknown');
  }
  return [...new Set(errors)];
}
