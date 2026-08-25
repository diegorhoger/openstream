import { useCallback, useEffect, useReducer, useRef } from 'react';
import type { DragEvent, ReactElement } from 'react';
import {
  editorReducer,
  findControl,
  findPage,
  firstFreeCell,
  initialEditorState,
  ZOOM_LEVELS,
} from './studio/editor.ts';
import { messagesFor, formatMessage } from './i18n/catalog.ts';
import {
  BRIDGE_UNAVAILABLE_TOKEN,
  tauriBridge,
  type StudioBridge,
} from './studio/bridge.ts';
import { handleCanvasKeys, handleGlobalKeys, isTypingTarget, type EditorCommand } from './studio/keyboard.ts';
import { validateDeckDocument, validateProfileDocument } from './studio/decode.ts';
import type { ControlKind, InteractionPolicy, StudioOp } from './studio/types.ts';
import { renderStudio } from './studio/views/studio-view.ts';

/** Grid stride math shared with the canvas view (8 px spacing rhythm). */
const CELL_GAP_PX = 8;

function tokenOf(error: unknown): string {
  if (typeof error === 'string') {
    return error;
  }
  if (typeof error === 'object' && error !== null && 'token' in error) {
    const token = (error as { token: unknown }).token;
    if (typeof token === 'string') {
      return token;
    }
  }
  if (error instanceof Error && error.message === BRIDGE_UNAVAILABLE_TOKEN) {
    return BRIDGE_UNAVAILABLE_TOKEN;
  }
  return 'unknown';
}

/**
 * OpenStream Studio (issue #17): accessible visual deck editor.
 *
 * App owns interaction glue only. Every structural change flows to the Rust
 * editing service as a typed op and this component adopts the authoritative
 * snapshot it returns; the reducer never mutates documents locally. All
 * markup composition lives in `renderStudio`, which is also what the
 * executable accessibility contract renders — there is no second path.
 */
export function App(): ReactElement {
  const [state, dispatch] = useReducer(editorReducer, initialEditorState);
  const stateRef = useRef(state);
  stateRef.current = state;
  const bridgeRef = useRef<StudioBridge>(tauriBridge);

  useEffect(() => {
    let cancelled = false;
    bridgeRef.current
      .load()
      .then((result) => {
        if (cancelled) {
          return;
        }
        // Fail closed client-side: refuse snapshots violating the v1
        // contract before anything renders them.
        const errors = [
          ...result.snapshot.decks.flatMap(validateDeckDocument),
          ...result.snapshot.profiles.flatMap(validateProfileDocument),
        ];
        if (errors.length > 0) {
          dispatch({ type: 'load-failed', token: 'invalid_workspace' });
          return;
        }
        dispatch({ type: 'loaded', result });
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          dispatch({ type: 'load-failed', token: tokenOf(error) });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const sendOp = useCallback(async (op: StudioOp): Promise<void> => {
    try {
      const outcome = await bridgeRef.current.apply(op);
      dispatch({ type: 'applied', outcome });
    } catch (error: unknown) {
      dispatch({ type: 'op-rejected', token: tokenOf(error) });
    }
  }, []);

  const runCommand = useCallback(
    async (command: EditorCommand): Promise<void> => {
      switch (command.kind) {
        case 'op':
          await sendOp(command.op);
          break;
        case 'undo':
        case 'redo':
          try {
            const outcome =
              command.kind === 'undo'
                ? await bridgeRef.current.undo()
                : await bridgeRef.current.redo();
            dispatch({ type: 'applied', outcome });
          } catch (error: unknown) {
            dispatch({ type: 'op-rejected', token: tokenOf(error) });
          }
          break;
      }
    },
    [sendOp],
  );

  useEffect(() => {
    const handler = (event: KeyboardEvent): void => {
      const target = event.target;
      const typing = target instanceof HTMLElement && isTypingTarget(target.tagName);
      const input = {
        key: event.key,
        ctrl: event.ctrlKey,
        shift: event.shiftKey,
        alt: event.altKey,
      };
      const globalOutcome = handleGlobalKeys(stateRef.current, input, typing);
      if (globalOutcome !== null) {
        event.preventDefault();
        for (const command of globalOutcome.commands) {
          void runCommand(command);
        }
        for (const action of globalOutcome.actions) {
          dispatch(action);
        }
      }
    };
    document.addEventListener('keydown', handler as EventListener);
    return () => document.removeEventListener('keydown', handler as EventListener);
  }, [runCommand]);

  const onCanvasKeyDown = useCallback(
    (event: {
      key: string;
      ctrlKey: boolean;
      shiftKey: boolean;
      altKey: boolean;
      preventDefault(): void;
      target: EventTarget | null;
    }): void => {
      const target = event.target;
      const typing = target instanceof HTMLElement && isTypingTarget(target.tagName);
      if (typing) {
        return;
      }
      const input = {
        key: event.key,
        ctrl: event.ctrlKey,
        shift: event.shiftKey,
        alt: event.altKey,
      };
      // Global chords work inside the canvas too (Ctrl+Z on a focused key).
      const globalOutcome = handleGlobalKeys(stateRef.current, input, typing);
      const outcome = globalOutcome ?? handleCanvasKeys(stateRef.current, input);
      if (outcome === null) {
        return;
      }
      // Consumed bindings suppress native button activation so Enter/Space
      // lift and drop instead of double-firing clicks.
      event.preventDefault();
      for (const command of outcome.commands) {
        void runCommand(command);
      }
      for (const action of outcome.actions) {
        dispatch(action);
      }
    },
    [runCommand],
  );

  const messages = messagesFor(state.locale);

  const callbacks = {
    // Toolbar
    onUndo: () => void runCommand({ kind: 'undo' }),
    onRedo: () => void runCommand({ kind: 'redo' }),
    onZoom: (direction: 'in' | 'out' | 'reset') => dispatch({ type: 'zoom', direction }),
    onLocale: (locale: import('./i18n/catalog.ts').LocaleId) =>
      dispatch({ type: 'locale-changed', locale }),
    onNewDeck: () => {
      void sendOp({
        type: 'create_deck',
        title: messages['studio.toolbar.newDeck'],
        folder_path: '',
      });
    },
    onNewProfile: () => {
      void sendOp({ type: 'create_profile', name: messages['studio.toolbar.newProfile'] });
    },

    // Canvas
    onSelectControl: (pageId: string, controlId: string, label: string): void => {
      dispatch({
        type: 'select',
        selection: { kind: 'control', pageId, controlId },
        announceName: label,
      });
    },
    onCanvasKeyDown,
    onControlDragStart: (controlId: string, event: DragEvent<HTMLButtonElement>): void => {
      event.dataTransfer.setData('text/plain', controlId);
      event.dataTransfer.effectAllowed = 'move';
    },
    onGridDrop: (event: DragEvent<HTMLDivElement>): void => {
      const controlId = event.dataTransfer.getData('text/plain');
      const page = findPage(stateRef.current.snapshot, stateRef.current.currentPageId);
      const control = findControl(stateRef.current.snapshot, controlId)?.control;
      if (controlId.length === 0 || page === null || control === undefined || control === null) {
        return;
      }
      const zoom = ZOOM_LEVELS[stateRef.current.zoomIndex] ?? 1;
      const stride = 88 * zoom + CELL_GAP_PX;
      const rect = event.currentTarget.getBoundingClientRect();
      const x = Math.min(
        Math.max(0, Math.floor((event.clientX - rect.left) / stride)),
        page.grid.columns - control.geometry.width,
      );
      const y = Math.min(
        Math.max(0, Math.floor((event.clientY - rect.top) / stride)),
        page.grid.rows - control.geometry.height,
      );
      if (x === control.geometry.x && y === control.geometry.y) {
        return;
      }
      void sendOp({ type: 'move_control', control_id: controlId, x, y });
      dispatch({
        type: 'announce',
        text: formatMessage(messages['studio.announce.dropped'], {
          name: control.label,
          x,
          y,
        }),
      });
    },
    onAddControl: (kind: ControlKind): void => {
      const page = findPage(stateRef.current.snapshot, stateRef.current.currentPageId);
      if (page === null) {
        return;
      }
      const width = 2;
      const height = 1;
      const cell = firstFreeCell(page, width, height);
      const kindLabel = messages[`studio.control.kind.${kind}` as keyof typeof messages];
      void sendOp({
        type: 'add_control',
        page_id: page.id,
        kind,
        x: cell.x,
        y: cell.y,
        width,
        height,
        label: `${kindLabel} ${page.controls.length + 1}`,
        policy: null as InteractionPolicy | null, // Rust applies the kind default.
      });
    },

    // Pages rail
    onPageSelect: (_deckId: string, pageId: string): void => {
      // Selection first (clears stale announcements), then open (announces).
      dispatch({ type: 'select', selection: { kind: 'page', pageId } });
      dispatch({ type: 'open-page', pageId });
    },
    onPageAdd: (deckId: string): void => {
      void sendOp({ type: 'add_page', deck_id: deckId });
    },
    onPageMove: (deckId: string, pageId: string, toIndex: number): void => {
      void sendOp({ type: 'reorder_page', deck_id: deckId, page_id: pageId, to_index: toIndex });
    },
    onPageDelete: (deckId: string, pageId: string): void => {
      void sendOp({ type: 'remove_page', deck_id: deckId, page_id: pageId });
    },

    // Decks & folders
    onDeckSelect: (deckId: string): void => {
      const deck = stateRef.current.snapshot.decks.find((document) => document.deck.id === deckId);
      const firstPageId = deck?.deck.pages[0]?.id ?? null;
      if (firstPageId !== null && firstPageId !== undefined) {
        dispatch({ type: 'open-page', pageId: firstPageId });
      }
      dispatch({
        type: 'select',
        selection: { kind: 'deck', deckId },
        announceName: deck?.deck.title ?? '',
      });
    },
    onDeckFolderChange: (deckId: string, folderPath: string): void => {
      const deck = stateRef.current.snapshot.decks.find((document) => document.deck.id === deckId);
      if (deck !== undefined && deck.deck.folder_path !== folderPath) {
        void sendOp({ type: 'move_deck_to_folder', deck_id: deckId, folder_path: folderPath });
      }
    },
    onDeckDelete: (deckId: string): void => {
      void sendOp({ type: 'delete_deck', deck_id: deckId });
    },

    // Profiles panel
    onProfileSelect: (profileId: string): void => {
      const profile = stateRef.current.snapshot.profiles.find(
        (document) => document.profile.id === profileId,
      );
      dispatch({
        type: 'select',
        selection: { kind: 'profile', profileId },
        announceName: profile?.profile.name ?? '',
      });
    },
    onProfileAddDeck: (profileId: string, deckId: string): void => {
      void sendOp({ type: 'profile_add_deck', profile_id: profileId, deck_id: deckId });
    },
    onProfileMoveDeck: (profileId: string, deckId: string, toIndex: number): void => {
      void sendOp({
        type: 'profile_move_deck',
        profile_id: profileId,
        deck_id: deckId,
        to_index: toIndex,
      });
    },
    onProfileRemoveDeck: (profileId: string, deckId: string): void => {
      void sendOp({ type: 'profile_remove_deck', profile_id: profileId, deck_id: deckId });
    },
    onProfileDelete: (profileId: string): void => {
      void sendOp({ type: 'delete_profile', profile_id: profileId });
    },

    // Inspector
    onControlLabel: (controlId: string, label: string): void => {
      const current = findControl(stateRef.current.snapshot, controlId)?.control;
      if (
        current !== undefined &&
        current !== null &&
        current.label !== label &&
        label.trim().length > 0
      ) {
        void sendOp({ type: 'set_control_label', control_id: controlId, label });
      }
    },
    onControlKind: (controlId: string, kind: ControlKind): void => {
      void sendOp({ type: 'set_control_kind', control_id: controlId, kind });
    },
    onControlPolicy: (controlId: string, policy: InteractionPolicy | null): void => {
      void sendOp({ type: 'set_control_policy', control_id: controlId, policy });
    },
    onControlEnabled: (controlId: string, enabled: boolean): void => {
      void sendOp({ type: 'set_control_enabled', control_id: controlId, enabled });
    },
    onControlGeometryPatch: (
      controlId: string,
      patch: { x?: number; y?: number; width?: number; height?: number },
    ): void => {
      const control = findControl(stateRef.current.snapshot, controlId)?.control;
      if (control === undefined || control === null) {
        return;
      }
      if (
        (patch.x !== undefined && patch.x !== control.geometry.x) ||
        (patch.y !== undefined && patch.y !== control.geometry.y)
      ) {
        void sendOp({
          type: 'move_control',
          control_id: controlId,
          x: patch.x ?? control.geometry.x,
          y: patch.y ?? control.geometry.y,
        });
      }
      if (
        (patch.width !== undefined && patch.width !== control.geometry.width) ||
        (patch.height !== undefined && patch.height !== control.geometry.height)
      ) {
        void sendOp({
          type: 'resize_control',
          control_id: controlId,
          width: patch.width ?? control.geometry.width,
          height: patch.height ?? control.geometry.height,
        });
      }
    },
    onControlDelete: (controlId: string): void => {
      void sendOp({ type: 'remove_control', control_id: controlId });
    },
    onPageGrid: (deckId: string, pageId: string, columns: number, rows: number): void => {
      const page = findPage(stateRef.current.snapshot, pageId);
      if (page !== null && (page.grid.columns !== columns || page.grid.rows !== rows)) {
        void sendOp({ type: 'resize_grid', deck_id: deckId, page_id: pageId, columns, rows });
      }
    },
    onDeckTitle: (deckId: string, title: string): void => {
      const deck = stateRef.current.snapshot.decks.find((document) => document.deck.id === deckId);
      if (deck !== undefined && deck.deck.title !== title && title.trim().length > 0) {
        void sendOp({ type: 'rename_deck', deck_id: deckId, title });
      }
    },
    onDeckFolder: (deckId: string, folderPath: string): void => {
      const deck = stateRef.current.snapshot.decks.find((document) => document.deck.id === deckId);
      if (deck !== undefined && deck.deck.folder_path !== folderPath) {
        void sendOp({ type: 'move_deck_to_folder', deck_id: deckId, folder_path: folderPath });
      }
    },
    onProfileName: (profileId: string, name: string): void => {
      const profile = stateRef.current.snapshot.profiles.find(
        (document) => document.profile.id === profileId,
      );
      if (profile !== undefined && profile.profile.name !== name && name.trim().length > 0) {
        void sendOp({ type: 'rename_profile', profile_id: profileId, name });
      }
    },
  };

  return renderStudio(state, callbacks);
}
