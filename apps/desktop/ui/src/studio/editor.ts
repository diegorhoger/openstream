/**
 * Pure editor state for the Studio deck editor (issue #17).
 *
 * The reducer owns ONLY view concerns — selection, current page, zoom,
 * keyboard-lift state, announcements, and honest autosave/error surfacing.
 * Structural truth is never invented here: every mutation goes through the
 * Rust editing service as a {@link StudioOp}, and this state adopts the
 * authoritative snapshot that comes back.
 *
 * Keyboard drag-alternative lives here too: a control can be "lifted"
 * (Enter), nudged/resized with arrows, then dropped (Enter) or returned
 * (Escape). The drop builds exactly the same `move_control`/`resize_control`
 * op the pointer-drag path builds (parity asserted by tests).
 */

import { formatMessage, messagesFor, type LocaleId } from '../i18n/catalog.ts';
import { LIMITS, type Diagnostic } from './types.ts';
import type {
  ApplyOutcome,
  Control,
  DeckDocument,
  Geometry,
  LoadResult,
  Page,
  ProfileDocument,
  StudioOp,
  WorkspaceSnapshot,
} from './types.ts';

export const ZOOM_LEVELS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2] as const;
export const DEFAULT_ZOOM_INDEX = 2;

export type Selection =
  | { kind: 'control'; pageId: string; controlId: string }
  | { kind: 'page'; pageId: string }
  | { kind: 'deck'; deckId: string }
  | { kind: 'profile'; profileId: string };

export interface LiftState {
  controlId: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface EditorState {
  readonly phase: 'loading' | 'ready' | 'failed';
  /** Which main experience fills the window: authoring or live surface. */
  readonly mode: 'edit' | 'live';
  readonly loadErrorToken: string | null;
  readonly autosaveActive: boolean;
  readonly snapshot: WorkspaceSnapshot;
  readonly selection: Selection | null;
  readonly currentPageId: string | null;
  /** Index into ZOOM_LEVELS. */
  readonly zoomIndex: number;
  readonly lift: LiftState | null;
  /** Text queued into the polite live region (screen-reader announcements). */
  readonly announcement: string;
  /** Bumped per announcement so repeats of identical text still fire. */
  readonly announcementSeq: number;
  readonly locale: LocaleId;
  readonly canUndo: boolean;
  readonly canRedo: boolean;
  readonly saved: boolean;
  readonly saveError: string | null;
  readonly lastOpError: string | null;
  /** Latest non-blocking diagnostics (grid collisions) from the service. */
  readonly diagnostics: readonly Diagnostic[];
}

export const initialEditorState: EditorState = {
  phase: 'loading',
  mode: 'edit',
  loadErrorToken: null,
  autosaveActive: false,
  snapshot: { decks: [], profiles: [] },
  selection: null,
  currentPageId: null,
  zoomIndex: DEFAULT_ZOOM_INDEX,
  lift: null,
  announcement: '',
  announcementSeq: 0,
  locale: 'en-US',
  canUndo: false,
  canRedo: false,
  saved: true,
  saveError: null,
  lastOpError: null,
  diagnostics: [],
};

export type EditorAction =
  | { type: 'loaded'; result: LoadResult }
  | { type: 'load-failed'; token: string }
  | { type: 'applied'; outcome: ApplyOutcome }
  | { type: 'op-rejected'; token: string }
  | { type: 'mode-changed'; mode: 'edit' | 'live' }
  | { type: 'select'; selection: Selection | null; announceName?: string }
  | { type: 'open-page'; pageId: string }
  | { type: 'zoom'; direction: 'in' | 'out' | 'reset' }
  | { type: 'locale-changed'; locale: LocaleId }
  | { type: 'lift-begin'; pageId: string; controlId: string }
  | { type: 'lift-nudge'; dx: number; dy: number }
  | { type: 'lift-resize'; dw: number; dh: number }
  | { type: 'lift-cancel' }
  | { type: 'lift-dropped'; announcementText: string }
  | { type: 'announce'; text: string };

function findDeckOfPage(snapshot: WorkspaceSnapshot, pageId: string): DeckDocument | null {
  return (
    snapshot.decks.find((document) =>
      document.deck.pages.some((page) => page.id === pageId),
    ) ?? null
  );
}

export function findPage(
  snapshot: WorkspaceSnapshot,
  pageId: string | null,
): Page | null {
  if (pageId === null) {
    return null;
  }
  for (const document of snapshot.decks) {
    const page = document.deck.pages.find((candidate) => candidate.id === pageId);
    if (page !== undefined) {
      return page;
    }
  }
  return null;
}

export function findControl(
  snapshot: WorkspaceSnapshot,
  controlId: string | null,
): { control: Control; page: Page } | null {
  if (controlId === null) {
    return null;
  }
  for (const document of snapshot.decks) {
    for (const page of document.deck.pages) {
      const control = page.controls.find((candidate) => candidate.id === controlId);
      if (control !== undefined) {
        return { control, page };
      }
    }
  }
  return null;
}

/** First page of the given deck in ordinal order, or null when empty. */
export function firstPageOf(deck: DeckDocument): Page | null {
  return [...deck.deck.pages].sort((a, b) => a.ordinal - b.ordinal)[0] ?? null;
}

/**
 * Clamps a lifted control's top-left cell so the geometry stays inside the
 * page grid — arrow keys can never produce an invalid position.
 */
export function clampPosition(page: Page, width: number, height: number, x: number, y: number): { x: number; y: number } {
  const maxX = Math.max(0, page.grid.columns - width);
  const maxY = Math.max(0, page.grid.rows - height);
  return {
    x: Math.min(Math.max(0, Math.round(x)), maxX),
    y: Math.min(Math.max(0, Math.round(y)), maxY),
  };
}

/**
 * Clamps a lifted control's working extents to at least one cell and inside
 * the grid from its anchor position.
 */
export function clampExtent(page: Page, anchorX: number, anchorY: number, width: number, height: number): { width: number; height: number } {
  const maxWidth = page.grid.columns - anchorX;
  const maxHeight = page.grid.rows - anchorY;
  return {
    width: Math.min(Math.max(1, Math.round(width)), maxWidth),
    height: Math.min(Math.max(1, Math.round(height)), maxHeight),
  };
}

function geometryOf(control: Control): Geometry {
  return { ...control.geometry };
}

function selectionStillExists(snapshot: WorkspaceSnapshot, selection: Selection | null): boolean {
  if (selection === null) {
    return true;
  }
  switch (selection.kind) {
    case 'control':
      return findControl(snapshot, selection.controlId) !== null;
    case 'page':
      return findPage(snapshot, selection.pageId) !== null;
    case 'deck':
      return snapshot.decks.some((document) => document.deck.id === selection.deckId);
    case 'profile':
      return snapshot.profiles.some((document) => document.profile.id === selection.profileId);
  }
}

function adoptOutcome(state: EditorState, outcome: ApplyOutcome): EditorState {
  let currentPageId = state.currentPageId;
  let selection = state.selection;
  let lift = state.lift;
  if (!selectionStillExists(outcome.snapshot, selection)) {
    selection = null;
  }
  if (currentPageId === null || findPage(outcome.snapshot, currentPageId) === null) {
    const fallbackDeck = outcome.snapshot.decks[0];
    currentPageId = fallbackDeck ? (firstPageOf(fallbackDeck)?.id ?? null) : null;
  }
  if (lift !== null && findControl(outcome.snapshot, lift.controlId) === null) {
    lift = null;
  }
  return {
    ...state,
    snapshot: outcome.snapshot,
    canUndo: outcome.can_undo,
    canRedo: outcome.can_redo,
    saved: outcome.saved,
    saveError: outcome.save_error,
    lastOpError: null,
    selection,
    currentPageId,
    lift,
    diagnostics: outcome.diagnostics,
  };
}

export function editorReducer(state: EditorState, action: EditorAction): EditorState {
  switch (action.type) {
    case 'loaded': {
      const snapshot = action.result.snapshot;
      const fallbackDeck = snapshot.decks[0];
      const pageId = fallbackDeck ? (firstPageOf(fallbackDeck)?.id ?? null) : null;
      return {
        ...state,
        phase: 'ready',
        autosaveActive: action.result.autosave_active,
        snapshot,
        currentPageId: pageId,
      };
    }
    case 'load-failed':
      return { ...state, phase: 'failed', loadErrorToken: action.token };
    case 'mode-changed':
      return { ...state, mode: action.mode };
    case 'applied': {
      const next = adoptOutcome(state, action.outcome);
      return next;
    }
    case 'op-rejected':
      return { ...state, lastOpError: action.token };
    case 'select': {
      const messages = messagesFor(state.locale);
      const text =
        action.announceName === undefined
          ? ''
          : formatMessage(messages['studio.announce.selected'], {
              name: action.announceName,
            });
      return {
        ...state,
        selection: action.selection,
        announcement: text,
        announcementSeq: state.announcementSeq + 1,
        lift: null,
      };
    }
    case 'open-page': {
      const deck = findDeckOfPage(state.snapshot, action.pageId);
      const index = deck
        ? [...deck.deck.pages]
            .sort((a, b) => a.ordinal - b.ordinal)
            .findIndex((candidate) => candidate.id === action.pageId) + 1
        : 0;
      const messages = messagesFor(state.locale);
      return {
        ...state,
        currentPageId: action.pageId,
        selection: null,
        lift: null,
        announcement: formatMessage(messages['studio.announce.pageOpened'], { index }),
        announcementSeq: state.announcementSeq + 1,
      };
    }
    case 'zoom': {
      if (action.direction === 'reset') {
        return { ...state, zoomIndex: DEFAULT_ZOOM_INDEX };
      }
      const delta = action.direction === 'in' ? 1 : -1;
      const nextIndex = Math.min(
        ZOOM_LEVELS.length - 1,
        Math.max(0, state.zoomIndex + delta),
      );
      return { ...state, zoomIndex: nextIndex };
    }
    case 'locale-changed':
      return { ...state, locale: action.locale };
    case 'lift-begin': {
      const found = findControl(state.snapshot, action.controlId);
      if (found === null) {
        return state;
      }
      const geometry = geometryOf(found.control);
      const messages = messagesFor(state.locale);
      return {
        ...state,
        selection: { kind: 'control', pageId: found.page.id, controlId: found.control.id },
        lift: {
          controlId: found.control.id,
          x: geometry.x,
          y: geometry.y,
          width: geometry.width,
          height: geometry.height,
        },
        announcement: formatMessage(messages['studio.announce.lifted'], {
          name: found.control.label,
          hint: messages['studio.lift.hint'],
        }),
        announcementSeq: state.announcementSeq + 1,
      };
    }
    case 'lift-nudge':
    case 'lift-resize': {
      const page = findPage(state.snapshot, state.currentPageId);
      const lift = state.lift;
      if (page === null || lift === null) {
        return state;
      }
      if (action.type === 'lift-nudge') {
        const moved = clampPosition(page, lift.width, lift.height, lift.x + action.dx, lift.y + action.dy);
        return { ...state, lift: { ...lift, ...moved } };
      }
      const extent = clampExtent(page, lift.x, lift.y, lift.width + action.dw, lift.height + action.dh);
      return { ...state, lift: { ...lift, ...extent } };
    }
    case 'lift-cancel': {
      if (state.lift === null) {
        return state;
      }
      const messages = messagesFor(state.locale);
      return {
        ...state,
        lift: null,
        announcement: formatMessage(messages['studio.announce.canceled'], {}),
        announcementSeq: state.announcementSeq + 1,
      };
    }
    case 'lift-dropped':
      return {
        ...state,
        lift: null,
        announcement: action.announcementText,
        announcementSeq: state.announcementSeq + 1,
      };
    case 'announce':
      return {
        ...state,
        announcement: action.text,
        announcementSeq: state.announcementSeq + 1,
      };
  }
}

/**
 * Builds the authoritative op for dropping the currently lifted control at
 * its working position/size. Returns null when nothing is lifted or when
 * neither position nor size changed (no-op drop).
 */
export function buildLiftDropOps(state: EditorState): StudioOp[] {
  const { lift } = state;
  if (lift === null) {
    return [];
  }
  const found = findControl(state.snapshot, lift.controlId);
  if (found === null) {
    return [];
  }
  const ops: StudioOp[] = [];
  if (found.control.geometry.width !== lift.width || found.control.geometry.height !== lift.height) {
    ops.push({
      type: 'resize_control',
      control_id: lift.controlId,
      width: lift.width,
      height: lift.height,
    });
  }
  if (found.control.geometry.x !== lift.x || found.control.geometry.y !== lift.y) {
    ops.push({
      type: 'move_control',
      control_id: lift.controlId,
      x: lift.x,
      y: lift.y,
    });
  }
  return ops;
}

/** Builds the localized drop announcement for the lifted control. */
export function liftDropAnnouncement(state: EditorState): string {
  const lift = state.lift;
  if (lift === null) {
    return '';
  }
  const messages = messagesFor(state.locale);
  const found = findControl(state.snapshot, lift.controlId);
  return formatMessage(messages['studio.announce.dropped'], {
    name: found?.control.label ?? '',
    x: lift.x,
    y: lift.y,
  });
}

/**
 * Localizes a closed-vocabulary refusal token from the Rust service into a
 * full sentence for the inspector's alert line. Unknown tokens still get an
 * honest generic refusal sentence — nothing fails silently.
 */
export function describeOpError(token: string, locale: LocaleId): string {
  const messages = messagesFor(locale);
  const prefix = `${messages['studio.error.prefix']} `;
  const textFieldNames: Readonly<Record<string, keyof typeof messages>> = {
    title: 'studio.inspector.titleField',
    label: 'studio.inspector.labelField',
    name: 'studio.inspector.nameField',
  };
  if (token.startsWith('text_out_of_range:')) {
    const field = token.slice('text_out_of_range:'.length);
    const key = textFieldNames[field];
    const fieldLabel =
      key === undefined ? field : String(messages[key]);
    return (
      prefix +
      formatMessage(messages['studio.error.text_out_of_range'], {
        field: fieldLabel,
        max: LIMITS.maxTextBytes,
      })
    );
  }
  if (token.startsWith('geometry_outside_grid:')) {
    return (
      prefix +
      formatMessage(messages['studio.error.geometry_outside_grid'], {
        axis: token.slice('geometry_outside_grid:'.length),
      })
    );
  }
  if (token.startsWith('not_found:')) {
    return prefix + messages['studio.error.not_found'];
  }
  if (token.startsWith('invalid_id:')) {
    return prefix + messages['studio.error.invalid_id'];
  }
  if (token.startsWith('autosave_')) {
    return formatMessage(messages['studio.save.unavailable'], { token });
  }
  if (token.startsWith('invalid_hotkey:')) {
    return prefix + messages['studio.error.invalid_hotkey'];
  }
  if (token.startsWith('invalid_app_identity:')) {
    return prefix + messages['studio.error.invalid_app_identity'];
  }
  if (token.startsWith('conflicting_switch_rule:')) {
    return prefix + messages['studio.error.conflicting_switch_rule'];
  }
  if (token === 'foreign_switch_rule') {
    return prefix + messages['studio.error.conflicting_switch_rule'];
  }
  const directKeys = [
    'limit_exceeded',
    'ordinal_conflict',
    'duplicate_control',
    'duplicate_deck_ref',
    'policy_not_allowed',
    'zero_extent',
    'revision_overflow',
    'invalid_folder',
    'invalid_id',
    'deck_deleted',
  ] as const;
  if ((directKeys as readonly string[]).includes(token)) {
    const key = `studio.error.${token}` as keyof typeof messages;
    return prefix + messages[key];
  }
  return prefix + messages['studio.error.unknown'];
}

/** Convenience accessor used by views and tests. */
export function profilesOf(snapshot: WorkspaceSnapshot): ProfileDocument[] {
  return snapshot.profiles;
}

/**
 * Finds the first free top-left cell for a `width` x `height` placement,
 * scanning rows then columns and skipping occupied rectangles. Returns
 * {x: 0, y: 0} when the page is full — placement is still attempted (the
 * domain reports collisions as warnings, never rejections).
 */
export function firstFreeCell(page: Page, width: number, height: number): { x: number; y: number } {
  const occupies = (x: number, y: number): boolean =>
    page.controls.some((control) => {
      const geometry = control.geometry;
      return (
        x < geometry.x + geometry.width &&
        geometry.x < x + width &&
        y < geometry.y + geometry.height &&
        geometry.y < y + height
      );
    });
  for (let y = 0; y <= page.grid.rows - height; y += 1) {
    for (let x = 0; x <= page.grid.columns - width; x += 1) {
      if (!occupies(x, y)) {
        return { x, y };
      }
    }
  }
  return { x: 0, y: 0 };
}
