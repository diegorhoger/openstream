import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  clampExtent,
  clampPosition,
  describeOpError,
  editorReducer,
  findControl,
  findPage,
  firstFreeCell,
  initialEditorState,
  buildLiftDropOps,
  liftDropAnnouncement,
  type EditorState,
} from './editor.ts';
import { formatMessage, messagesFor } from '../i18n/catalog.ts';
import type { Page } from './types.ts';

/**
 * Editor reducer and helper contract (issue #17): view state adopts only
 * authoritative snapshots; keyboard-lift geometry clamps inside the grid so
 * the keyboard alternative can never propose an invalid placement; refusal
 * tokens localize into full honest sentences in both locales.
 */

const PAGE_ID = '018f6a1c-7b21-7002-9f31-000000000002';
const CONTROL_ID = '018f6a1c-7b21-7003-9f31-00000000c001';

function testPage(): Page {
  return {
    id: PAGE_ID,
    deck_id: '018f6a1c-7b21-7001-9f31-000000000001',
    ordinal: 0,
    grid: { columns: 8, rows: 4 },
    controls: [
      {
        id: CONTROL_ID,
        page_id: PAGE_ID,
        kind: 'button',
        geometry: { x: 0, y: 0, width: 2, height: 1 },
        label: 'Mute mic',
        policy: 'press',
        enabled: true,
      },
    ],
  };
}

function readyState(): EditorState {
  const snapshot = {
    decks: [
      {
        schema_version: { major: 1, minor: 0 },
        deck: {
          id: '018f6a1c-7b21-7001-9f31-000000000001',
          workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
          title: 'Live scene',
          revision: 3,
          folder_path: '',
          deleted_at: null,
          pages: [testPage()],
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

describe('editor reducer', () => {
  it('starts loading with an empty workspace', () => {
    assert.equal(initialEditorState.phase, 'loading');
    assert.deepEqual(initialEditorState.snapshot.decks, []);
  });

  it('adopts the loaded snapshot and opens the first deck page', () => {
    const state = readyState();
    assert.equal(state.phase, 'ready');
    assert.equal(state.autosaveActive, true);
    assert.equal(state.currentPageId, PAGE_ID);
  });

  it('applied outcomes replace the snapshot and refresh undo flags honestly', () => {
    let state = readyState();
    state = editorReducer(state, {
      type: 'applied',
      outcome: {
        snapshot: { decks: [], profiles: [] },
        can_undo: true,
        can_redo: false,
        saved: false,
        save_error: 'autosave_refused',
        diagnostics: [],
      },
    });
    assert.deepEqual(state.snapshot.decks, []);
    assert.equal(state.canUndo, true);
    assert.equal(state.saved, false);
    assert.equal(state.saveError, 'autosave_refused');
    // The current page vanished with its deck; selection reconciles to null.
    assert.equal(state.currentPageId, null);
    assert.equal(state.selection, null);
  });

  it('select announces the chosen control by name', () => {
    let state = readyState();
    state = editorReducer(state, {
      type: 'select',
      selection: { kind: 'control', pageId: PAGE_ID, controlId: CONTROL_ID },
      announceName: 'Mute mic',
    });
    assert.equal(
      state.announcement,
      formatMessage(messagesFor('en-US')['studio.announce.selected'], { name: 'Mute mic' }),
    );
  });

  it('zoom steps stay within the documented levels and reset works', () => {
    let state = readyState();
    for (let i = 0; i < 10; i += 1) {
      state = editorReducer(state, { type: 'zoom', direction: 'in' });
    }
    assert.equal(state.zoomIndex, 6, 'zoom caps at the maximum level');
    state = editorReducer(state, { type: 'zoom', direction: 'reset' });
    assert.equal(state.zoomIndex, 2);
  });

  it('lift nudge clamps at the grid edges instead of leaving the page', () => {
    let state = readyState();
    state = editorReducer(state, {
      type: 'lift-begin',
      pageId: PAGE_ID,
      controlId: CONTROL_ID,
    });
    for (let i = 0; i < 20; i += 1) {
      state = editorReducer(state, { type: 'lift-nudge', dx: -1, dy: -1 });
    }
    assert.equal(state.lift?.x, 0);
    assert.equal(state.lift?.y, 0);
    for (let i = 0; i < 20; i += 1) {
      state = editorReducer(state, { type: 'lift-nudge', dx: 1, dy: 1 });
    }
    // width 2 on 8 columns -> max x = 6; height 1 on 4 rows -> max y = 3.
    assert.equal(state.lift?.x, 6);
    assert.equal(state.lift?.y, 3);
  });

  it('lift resize keeps extents >= 1 and inside the grid', () => {
    let state = readyState();
    state = editorReducer(state, {
      type: 'lift-begin',
      pageId: PAGE_ID,
      controlId: CONTROL_ID,
    });
    for (let i = 0; i < 5; i += 1) {
      state = editorReducer(state, { type: 'lift-resize', dw: -1, dh: -1 });
    }
    assert.ok((state.lift?.width ?? 0) >= 1);
    assert.ok((state.lift?.height ?? 0) >= 1);
    for (let i = 0; i < 20; i += 1) {
      state = editorReducer(state, { type: 'lift-resize', dw: 1, dh: 1 });
    }
    assert.equal(state.lift?.width, 8); // anchored at x=0
    assert.equal(state.lift?.height, 4);
  });

  it('drop builds exactly the ops that changed and none that did not', () => {
    let state = readyState();
    state = editorReducer(state, {
      type: 'lift-begin',
      pageId: PAGE_ID,
      controlId: CONTROL_ID,
    });
    state = editorReducer(state, { type: 'lift-nudge', dx: 3, dy: 2 });
    const ops = buildLiftDropOps(state);
    assert.deepEqual(ops, [{ type: 'move_control', control_id: CONTROL_ID, x: 3, y: 2 }]);
    // Dropping without moving anything is a no-op.
    const idle = readyState();
    const liftedIdle = editorReducer(idle, {
      type: 'lift-begin',
      pageId: PAGE_ID,
      controlId: CONTROL_ID,
    });
    assert.deepEqual(buildLiftDropOps(liftedIdle), []);
    assert.equal(
      liftDropAnnouncement(state),
      formatMessage(messagesFor('en-US')['studio.announce.dropped'], {
        name: 'Mute mic',
        x: 3,
        y: 2,
      }),
    );
  });

  it('refusals localize into complete sentences in both locales', () => {
    const en = messagesFor('en-US');
    const pt = messagesFor('pt-BR');
    assert.equal(
      describeOpError('text_out_of_range:title', 'en-US'),
      `${en['studio.error.prefix']} Title must be 1–256 characters.`,
    );
    assert.equal(
      describeOpError('text_out_of_range:title', 'pt-BR'),
      `${pt['studio.error.prefix']} Título deve ter de 1 a 256 caracteres.`,
    );
    assert.ok(describeOpError('geometry_outside_grid:x', 'pt-BR').includes('(x)'));
    assert.ok(describeOpError('limit_exceeded', 'en-US').includes('size limit'));
    assert.ok(
      describeOpError('mystery_token', 'en-US').includes(en['studio.error.unknown']),
      'unknown tokens still produce an honest sentence',
    );
  });
});

describe('grid helpers', () => {
  it('clampPosition bounds the top-left cell for the working size', () => {
    const page = testPage();
    assert.deepEqual(clampPosition(page, 2, 1, -4, -4), { x: 0, y: 0 });
    assert.deepEqual(clampPosition(page, 2, 1, 99, 99), { x: 6, y: 3 });
    assert.deepEqual(clampPosition(page, 2, 1, 3, 2), { x: 3, y: 2 });
  });

  it('clampExtent keeps sizes between one cell and the remaining space', () => {
    const page = testPage();
    assert.deepEqual(clampExtent(page, 0, 0, 0, 0), { width: 1, height: 1 });
    assert.deepEqual(clampExtent(page, 0, 0, 100, 100), { width: 8, height: 4 });
    assert.deepEqual(clampExtent(page, 6, 3, 10, 10), { width: 2, height: 1 });
  });

  it('firstFreeCell skips occupied rectangles row-major', () => {
    let page = testPage();
    assert.deepEqual(firstFreeCell(page, 2, 1), { x: 2, y: 0 });
    page = {
      ...page,
      controls: [
        ...page.controls,
        {
          id: '018f6a1c-7b21-7003-9f31-00000000c002',
          page_id: PAGE_ID,
          kind: 'toggle' as const,
          geometry: { x: 2, y: 0, width: 6, height: 1 },
          label: 'row filler',
          policy: 'toggle' as const,
          enabled: true,
        },
      ],
    };
    assert.deepEqual(firstFreeCell(page, 2, 1), { x: 0, y: 1 }, 'first row is full');
  });

  it('find helpers resolve entities across decks', () => {
    const state = readyState();
    assert.equal(findPage(state.snapshot, PAGE_ID)?.id, PAGE_ID);
    assert.equal(findControl(state.snapshot, CONTROL_ID)?.control.label, 'Mute mic');
    assert.equal(findControl(state.snapshot, 'missing'), null);
  });
});
