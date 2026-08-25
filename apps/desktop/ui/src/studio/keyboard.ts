/**
 * Keyboard layer for the Studio editor (issue #17).
 *
 * Pure functions only: given the current {@link EditorState} and a plain
 * key-event descriptor, they return the state actions and editor commands
 * to run — or null when the key is not bound. App.tsx translates these into
 * dispatches and bridge calls, calling `preventDefault()` whenever an
 * outcome exists so native button activation never double-fires behind an
 * intentional binding.
 *
 * DESIGN_SYSTEM.md: "Drag/drop always has keyboard alternatives." Every
 * pointer-drag operation has a binding here (move/resize controls via
 * lift+nudge+drop; everything else ships as focusable buttons), and the
 * test suite asserts the keyboard path produces byte-identical ops to the
 * drag path.
 */

import { messagesFor } from '../i18n/catalog.ts';
import {
  buildLiftDropOps,
  findControl,
  liftDropAnnouncement,
  type EditorAction,
  type EditorState,
  type Selection,
  ZOOM_LEVELS,
} from './editor.ts';
import type { StudioOp } from './types.ts';

/** Plain descriptor of the DOM KeyboardEvent fields the bindings read. */
export interface KeyInput {
  readonly key: string;
  readonly ctrl: boolean;
  readonly shift: boolean;
  readonly alt: boolean;
}

/** Something the shell must execute on behalf of the keyboard. */
export type EditorCommand =
  | { readonly kind: 'op'; readonly op: StudioOp }
  | { readonly kind: 'undo' }
  | { readonly kind: 'redo' };

export interface KeyboardOutcome {
  readonly actions: readonly EditorAction[];
  readonly commands: readonly EditorCommand[];
}

const ARROW_DELTAS: Readonly<Record<string, { dx: number; dy: number }>> = {
  ArrowUp: { dx: 0, dy: -1 },
  ArrowDown: { dx: 0, dy: 1 },
  ArrowLeft: { dx: -1, dy: 0 },
  ArrowRight: { dx: 1, dy: 0 },
};

/**
 * True while the focus sits in a text field: global single-key semantics
 * must not fire there. Shortcut chords are still allowed by callers that
 * want them, but this guard keeps typing honest.
 */
export function isTypingTarget(tagName: string | undefined): boolean {
  const tag = tagName?.toLowerCase();
  return tag === 'input' || tag === 'select' || tag === 'textarea';
}

function selectionControlId(state: EditorState): string | null {
  const selection: Selection | null = state.selection;
  return selection !== null && selection.kind === 'control' ? selection.controlId : null;
}

function selectedControl(state: EditorState): { label: string; id: string } | null {
  const controlId = selectionControlId(state);
  if (controlId === null) {
    return null;
  }
  return findControl(state.snapshot, controlId)?.control ?? null;
}

/**
 * Bindings scoped to the deck canvas (and its controls). Handles the full
 * keyboard alternative for dragging:
 *   Enter / Space  lift (when idle) or drop (while lifted)
 *   Arrows         move lifted control one cell
 *   Shift+Arrows   grow lifted control by one cell per axis
 *   Alt+Shift+Ar.  shrink lifted control by one cell per axis
 *   Escape         cancel the move
 *   Delete         remove the selected control
 */
export function handleCanvasKeys(state: EditorState, event: KeyInput): KeyboardOutcome | null {
  if (state.phase !== 'ready') {
    return null;
  }

  // While lifted: the arrows belong to the drag-alternative.
  if (state.lift !== null) {
    const delta = ARROW_DELTAS[event.key];
    if (delta !== undefined) {
      if (event.shift) {
        const shrinkFactor = event.alt ? -1 : 1;
        return {
          actions: [
            {
              type: 'lift-resize',
              dw: delta.dx * shrinkFactor,
              dh: delta.dy * shrinkFactor,
            },
          ],
          commands: [],
        };
      }
      return {
        actions: [{ type: 'lift-nudge', ...delta }],
        commands: [],
      };
    }
    if (event.key === 'Enter' || event.key === ' ') {
      const ops = buildLiftDropOps(state);
      const announcement = liftDropAnnouncement(state);
      return {
        actions: [{ type: 'lift-dropped', announcementText: announcement }],
        commands: ops.map((op) => ({ kind: 'op', op }) as const),
      };
    }
    if (event.key === 'Escape') {
      return { actions: [{ type: 'lift-cancel' }], commands: [] };
    }
    return null;
  }

  // Not lifted: Enter/Space lifts the selected control, Delete removes it.
  const control = selectedControl(state);
  if (control !== null && (event.key === 'Enter' || event.key === ' ')) {
    const pageId = state.selection?.kind === 'control' ? state.selection.pageId : null;
    if (pageId === null) {
      return null;
    }
    return {
      actions: [{ type: 'lift-begin', pageId, controlId: control.id }],
      commands: [],
    };
  }
  if (control !== null && !event.ctrl && (event.key === 'Delete' || event.key === 'Backspace')) {
    return {
      actions: [],
      commands: [
        { kind: 'op', op: { type: 'remove_control', control_id: control.id } },
      ],
    };
  }
  return null;
}

/**
 * Global bindings: undo/redo and zoom chords. Skipped while the user types
 * in inspector fields so text editing keeps native behavior.
 */
export function handleGlobalKeys(
  state: EditorState,
  event: KeyInput,
  typingTarget: boolean,
): KeyboardOutcome | null {
  if (state.phase !== 'ready' || typingTarget || !event.ctrl) {
    return null;
  }
  if (event.key.toLowerCase() === 'z') {
    if (event.shift) {
      return { actions: [], commands: [{ kind: 'redo' }] };
    }
    return { actions: [], commands: [{ kind: 'undo' }] };
  }
  if (event.key.toLowerCase() === 'y') {
    return { actions: [], commands: [{ kind: 'redo' }] };
  }
  if (event.key === '=' || event.key === '+') {
    return {
      actions: [{ type: 'zoom', direction: ZOOM_LEVELS.length > 1 ? 'in' : 'out' }],
      commands: [],
    };
  }
  if (event.key === '-') {
    return { actions: [{ type: 'zoom', direction: 'out' }], commands: [] };
  }
  return null;
}

/** Localized hint shown under the canvas while a control is lifted. */
export function liftHintText(state: EditorState): string {
  return messagesFor(state.locale)['studio.lift.hint'];
}
