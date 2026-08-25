import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  editorReducer,
  initialEditorState,
  type EditorState,
} from './editor.ts';
import {
  handleCanvasKeys,
  handleGlobalKeys,
  isTypingTarget,
  type KeyInput,
} from './keyboard.ts';
import type { StudioOp } from './types.ts';

/**
 * Keyboard-path parity contract (issue #17, DESIGN_SYSTEM.md "Drag/drop
 * always has keyboard alternatives").
 *
 * For every pointer-drag operation the editor supports, this suite drives
 * the KEYBOARD path through the pure binding layer and asserts it emits the
 * byte-identical op the pointer/drag path emits. A keyboard user can do
 * everything a mouse user can, and both converge on the exact same
 * authoritative op vocabulary.
 */

const PAGE_ID = '018f6a1c-7b21-7002-9f31-000000000002';
const DECK_ID = '018f6a1c-7b21-7001-9f31-000000000001';
const CONTROL_ID = '018f6a1c-7b21-7003-9f31-00000000c001';

function readyState(): EditorState {
  const snapshot = {
    decks: [
      {
        schema_version: { major: 1, minor: 0 },
        deck: {
          id: DECK_ID,
          workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
          title: 'Live scene',
          revision: 3,
          folder_path: '',
          deleted_at: null,
          pages: [
            {
              id: PAGE_ID,
              deck_id: DECK_ID,
              ordinal: 0,
              grid: { columns: 8, rows: 4 },
              controls: [
                {
                  id: CONTROL_ID,
                  page_id: PAGE_ID,
                  kind: 'button' as const,
                  geometry: { x: 0, y: 0, width: 2, height: 1 },
                  label: 'Mute mic',
                  policy: 'press' as const,
                  enabled: true,
                },
              ],
            },
          ],
        },
      },
    ],
    profiles: [],
  };
  return editorReducer(initialEditorState, {
    type: 'loaded',
    result: { snapshot, autosave_active: true },
  });
}

function selectedState(): EditorState {
  let state = readyState();
  state = editorReducer(state, {
    type: 'select',
    selection: { kind: 'control', pageId: PAGE_ID, controlId: CONTROL_ID },
    announceName: 'Mute mic',
  });
  return state;
}

function key(partial: Partial<KeyInput> & { key: string }): KeyInput {
  return { ctrl: false, shift: false, alt: false, ...partial };
}

function commandsOf(outcome: { commands: readonly unknown[] }): StudioOp[] {
  return outcome.commands
    .filter((command): command is { kind: 'op'; op: StudioOp } => (command as { kind: string }).kind === 'op')
    .map((command) => command.op);
}

describe('keyboard alternative for moving controls', () => {
  it('lift -> arrows -> drop emits the same move_control op as pointer drag+drop', () => {
    // Pointer path: onDrop computes cell (3,2) and dispatches this op.
    const pointerOp: StudioOp = { type: 'move_control', control_id: CONTROL_ID, x: 3, y: 2 };

    // Keyboard path: identical outcome via pure bindings.
    let state = selectedState();
    const lift = handleCanvasKeys(state, key({ key: 'Enter' }));
    assert.ok(lift, 'Enter lifts');
    for (const action of lift.actions) {
      state = editorReducer(state, action);
    }

    for (let i = 0; i < 3; i += 1) {
      const right = handleCanvasKeys(state, key({ key: 'ArrowRight' }));
      assert.ok(right);
      for (const action of right.actions) {
        state = editorReducer(state, action);
      }
    }
    for (let i = 0; i < 2; i += 1) {
      const down = handleCanvasKeys(state, key({ key: 'ArrowDown' }));
      assert.ok(down);
      for (const action of down.actions) {
        state = editorReducer(state, action);
      }
    }

    const drop = handleCanvasKeys(state, key({ key: 'Enter' }));
    assert.ok(drop);
    assert.deepEqual(commandsOf(drop), [pointerOp]);
  });

  it('Shift+arrows then Enter emit the same resize_control op as inspector resize', () => {
    const inspectorOp: StudioOp = { type: 'resize_control', control_id: CONTROL_ID, width: 4, height: 2 };

    let state = selectedState();
    const lift = handleCanvasKeys(state, key({ key: ' ' }));
    assert.ok(lift, 'Space also lifts');
    for (const action of lift.actions) {
      state = editorReducer(state, action);
    }
    for (let i = 0; i < 2; i += 1) {
      const growW = handleCanvasKeys(state, key({ key: 'ArrowRight', shift: true }));
      assert.ok(growW);
      for (const action of growW.actions) {
        state = editorReducer(state, action);
      }
    }
    const growH = handleCanvasKeys(state, key({ key: 'ArrowDown', shift: true }));
    assert.ok(growH);
    for (const action of growH.actions) {
      state = editorReducer(state, action);
    }
    const drop = handleCanvasKeys(state, key({ key: 'Enter' }));
    assert.ok(drop);
    assert.deepEqual(commandsOf(drop), [inspectorOp]);
  });

  it('Alt+Shift+arrows shrink and combine with moves in one drop like two ops', () => {
    // Pointer path performs resize + move as two ops (inspector patches).
    const expectedOps: StudioOp[] = [
      { type: 'resize_control', control_id: CONTROL_ID, width: 1, height: 1 },
      { type: 'move_control', control_id: CONTROL_ID, x: 5, y: 3 },
    ];

    let state = selectedState();
    const lift = handleCanvasKeys(state, key({ key: 'Enter' }));
    assert.ok(lift);
    for (const action of lift.actions) {
      state = editorReducer(state, action);
    }
    const shrink = handleCanvasKeys(state, key({ key: 'ArrowRight', shift: true, alt: true }));
    assert.ok(shrink);
    for (const action of shrink.actions) {
      state = editorReducer(state, action);
    }
    for (let i = 0; i < 5; i += 1) {
      const right = handleCanvasKeys(state, key({ key: 'ArrowRight' }));
      assert.ok(right);
      for (const action of right.actions) {
        state = editorReducer(state, action);
      }
    }
    for (let i = 0; i < 3; i += 1) {
      const down = handleCanvasKeys(state, key({ key: 'ArrowDown' }));
      assert.ok(down);
      for (const action of down.actions) {
        state = editorReducer(state, action);
      }
    }
    const drop = handleCanvasKeys(state, key({ key: 'Enter' }));
    assert.ok(drop);
    assert.deepEqual(commandsOf(drop), expectedOps);
  });

  it('Escape cancels without emitting any op', () => {
    let state = selectedState();
    const lift = handleCanvasKeys(state, key({ key: 'Enter' }));
    assert.ok(lift);
    for (const action of lift.actions) {
      state = editorReducer(state, action);
    }
    const cancel = handleCanvasKeys(state, key({ key: 'Escape' }));
    assert.ok(cancel);
    assert.deepEqual(cancel.commands, []);
    const cancelled = cancel.actions.reduce(
      (current, action) => editorReducer(current, action),
      state,
    );
    assert.equal(cancelled.lift, null);
  });

  it('Delete emits the same remove_control op as the inspector delete button', () => {
    const buttonOp: StudioOp = { type: 'remove_control', control_id: CONTROL_ID };
    const keyboardOutcome = handleCanvasKeys(selectedState(), key({ key: 'Delete' }));
    assert.ok(keyboardOutcome);
    assert.deepEqual(commandsOf(keyboardOutcome), [buttonOp]);
  });
});

describe('global keyboard shortcuts', () => {
  it('Ctrl+Z maps to undo exactly like the toolbar Undo button', () => {
    const outcome = handleGlobalKeys(readyState(), key({ key: 'z', ctrl: true }), false);
    assert.ok(outcome);
    assert.deepEqual(
      outcome.commands.map((command) => command.kind),
      ['undo'],
    );
  });

  it('Ctrl+Y and Ctrl+Shift+Z map to redo exactly like the toolbar Redo button', () => {
    for (const combo of [
      key({ key: 'y', ctrl: true }),
      key({ key: 'z', ctrl: true, shift: true }),
    ]) {
      const outcome = handleGlobalKeys(readyState(), combo, false);
      assert.ok(outcome);
      assert.deepEqual(
        outcome.commands.map((command) => command.kind),
        ['redo'],
      );
    }
  });

  it('typing targets keep native text editing: no shortcuts fire while typing', () => {
    const outcome = handleGlobalKeys(readyState(), key({ key: 'z', ctrl: true }), true);
    assert.equal(outcome, null);
  });

  it('zoom chords adjust zoom through the same reducer action as the buttons', () => {
    let state = readyState();
    const zoomIn = handleGlobalKeys(state, key({ key: '=', ctrl: true }), false);
    assert.ok(zoomIn);
    state = zoomIn.actions.reduce((current, action) => editorReducer(current, action), state);
    assert.equal(state.zoomIndex, 3);
    const zoomOut = handleGlobalKeys(state, key({ key: '-', ctrl: true }), false);
    assert.ok(zoomOut);
    state = zoomOut.actions.reduce((current, action) => editorReducer(current, action), state);
    assert.equal(state.zoomIndex, 2);
  });

  it('isTypingTarget recognizes every text-entry element', () => {
    for (const tag of ['input', 'select', 'textarea', 'INPUT']) {
      assert.equal(isTypingTarget(tag), true, tag);
    }
    assert.equal(isTypingTarget('button'), false);
    assert.equal(isTypingTarget(undefined), false);
  });
});

describe('bindings stay inert outside ready sessions', () => {
  it('no canvas or global bindings fire while loading', () => {
    const loading = initialEditorState;
    assert.equal(handleCanvasKeys(loading, key({ key: 'Enter' })), null);
    assert.equal(handleGlobalKeys(loading, key({ key: 'z', ctrl: true }), false), null);
  });

  it('unbound keys return null so native behavior continues untouched', () => {
    const state = readyState();
    assert.equal(handleCanvasKeys(state, key({ key: 'Tab' })), null);
    assert.equal(handleCanvasKeys(state, key({ key: 'a' })), null);
    assert.equal(handleGlobalKeys(state, key({ key: 'a', ctrl: true }), false), null);
  });
});
