/**
 * Studio shell composition (issue #17).
 *
 * Assembles the full editor from the panel/canvas/inspector views plus the
 * action bar, engine status, autosave status, and polite live region.
 * Markup stays in plain element factories so the accessibility contract
 * executes against exactly what ships. Layout follows DESIGN_SYSTEM.md:
 * header, main (side panels | canvas | inspector), footer — DOM order is
 * reading order and no manual tab-order overrides exist anywhere.
 */

import { createElement } from 'react';
import type { ReactElement } from 'react';
import {
  formatMessage,
  messagesFor,
  type LocaleId,
} from '../../i18n/catalog.ts';
import {
  describeOpError,
  ZOOM_LEVELS,
  type EditorState,
} from '../editor.ts';
import { renderCanvas, type CanvasCallbacks } from './canvas-view.ts';
import { renderInspector, type InspectorCallbacks } from './inspector-view.ts';
import {
  renderFoldersPanel,
  renderPagesRail,
  renderProfilesPanel,
  type FoldersPanelCallbacks,
  type PagesRailCallbacks,
  type ProfilesPanelCallbacks,
} from './panels-view.ts';

export interface StudioToolbarCallbacks {
  onUndo(): void;
  onRedo(): void;
  onZoom(direction: 'in' | 'out' | 'reset'): void;
  onLocale(locale: LocaleId): void;
  onNewDeck(): void;
  onNewProfile(): void;
  /** Switches the main experience between authoring and live surface. */
  onMode(mode: 'edit' | 'live'): void;
}

/** Everything the shell can ask the app to do. */
export interface StudioCallbacks
  extends StudioToolbarCallbacks,
    CanvasCallbacks,
    PagesRailCallbacks,
    FoldersPanelCallbacks,
    ProfilesPanelCallbacks,
    InspectorCallbacks {}

/** Localized save-status line derived from honest session flags. */
export function saveStatusText(state: EditorState): string {
  const messages = messagesFor(state.locale);
  if (!state.autosaveActive) {
    return formatMessage(messages['studio.save.unavailable'], {
      token: 'autosave_unavailable',
    });
  }
  if (state.saved) {
    return messages['studio.save.saved'];
  }
  return formatMessage(messages['studio.save.refused'], {
    token: state.saveError ?? 'autosave_refused',
  });
}

function toolbar(state: EditorState, callbacks: StudioToolbarCallbacks): ReactElement {
  const messages = messagesFor(state.locale);
  const zoom = ZOOM_LEVELS[state.zoomIndex] ?? 1;
  const localeButton = (locale: LocaleId, label: string): ReactElement =>
    createElement(
      'button',
      {
        key: locale,
        type: 'button',
        className:
          state.locale === locale
            ? 'control-button control-button-toggled'
            : 'control-button',
        'aria-pressed': state.locale === locale,
        lang: locale,
        onClick: () => callbacks.onLocale(locale),
      },
      label,
    );
  const modeButton = (mode: 'edit' | 'live', label: string): ReactElement =>
    createElement(
      'button',
      {
        key: mode,
        type: 'button',
        className:
          state.mode === mode ? 'control-button control-button-toggled' : 'control-button',
        'aria-pressed': state.mode === mode,
        onClick: () => callbacks.onMode(mode),
      },
      label,
    );

  return createElement(
    'div',
    { role: 'toolbar', 'aria-label': messages['studio.toolbar.label'], className: 'action-bar' },
    createElement(
      'button',
      {
        type: 'button',
        className: 'control-button',
        disabled: !state.canUndo,
        onClick: callbacks.onUndo,
      },
      messages['studio.toolbar.undo'],
    ),
    createElement(
      'button',
      {
        type: 'button',
        className: 'control-button',
        disabled: !state.canRedo,
        onClick: callbacks.onRedo,
      },
      messages['studio.toolbar.redo'],
    ),
    createElement('span', { className: 'toolbar-separator', 'aria-hidden': 'true' }),
    createElement(
      'button',
      { type: 'button', className: 'control-button', onClick: () => callbacks.onZoom('out') },
      messages['studio.toolbar.zoomOut'],
    ),
    createElement(
      'span',
      { className: 'zoom-level', 'aria-live': 'off' },
      formatMessage(messages['studio.zoom.level'], { percent: Math.round(zoom * 100) }),
    ),
    createElement(
      'button',
      { type: 'button', className: 'control-button', onClick: () => callbacks.onZoom('in') },
      messages['studio.toolbar.zoomIn'],
    ),
    createElement(
      'button',
      { type: 'button', className: 'control-button', onClick: () => callbacks.onZoom('reset') },
      messages['studio.toolbar.zoomReset'],
    ),
    createElement('span', { className: 'toolbar-separator', 'aria-hidden': 'true' }),
    createElement(
      'button',
      { type: 'button', className: 'control-button', onClick: callbacks.onNewDeck },
      messages['studio.toolbar.newDeck'],
    ),
    createElement(
      'button',
      { type: 'button', className: 'control-button', onClick: callbacks.onNewProfile },
      messages['studio.toolbar.newProfile'],
    ),
    createElement('span', { className: 'toolbar-separator', 'aria-hidden': 'true' }),
    createElement(
      'span',
      { className: 'language-group', role: 'group', 'aria-label': messages['studio.toolbar.language'] },
      localeButton('en-US', messages['studio.toolbar.language.en']),
      localeButton('pt-BR', messages['studio.toolbar.language.pt']),
    ),
    createElement('span', { className: 'toolbar-separator', 'aria-hidden': 'true' }),
    createElement(
      'span',
      { role: 'group', 'aria-label': messages['studio.toolbar.mode'] },
      modeButton('edit', messages['studio.mode.edit']),
      modeButton('live', messages['studio.mode.live']),
    ),
  );
}

function footer(state: EditorState): ReactElement {
  const messages = messagesFor(state.locale);
  // Zero-width-space alternation makes repeated announcements differ at the
  // text level so live regions re-speak identical consecutive texts.
  const announcement =
    state.announcement.length === 0
      ? ''
      : state.announcementSeq % 2 === 0
        ? state.announcement
        : `${state.announcement}\u200b`;
  return createElement(
    'footer',
    { className: 'shell-footer' },
    createElement(
      'section',
      { className: 'engine-status', 'aria-labelledby': 'engine-status-heading' },
      createElement(
        'h2',
        { id: 'engine-status-heading', className: 'engine-heading' },
        messages['engine.heading'],
      ),
      createElement(
        'p',
        { className: 'status-line' },
        createElement('span', {
          'aria-hidden': 'true',
          className: 'status-dot',
        }),
        createElement('span', { className: 'visually-hidden' }, messages['engine.status.label']),
        messages['engine.status.notConnected'],
      ),
    ),
    createElement('p', { className: 'save-status', role: 'status' }, saveStatusText(state)),
    createElement('p', { className: 'visually-hidden', 'aria-live': 'polite' }, announcement),
    createElement('p', { className: 'muted' }, messages['engine.body.muted']),
    createElement('p', { className: 'muted' }, messages['shell.footer.note']),
  );
}

/** The complete Studio document. */
export function renderStudioShell(
  state: EditorState,
  mainContent: readonly ReactElement[],
  toolbarCallbacks: StudioToolbarCallbacks,
): ReactElement {
  const messages = messagesFor(state.locale);
  return createElement(
    'div',
    { className: 'shell studio-shell' },
    createElement(
      'header',
      { className: 'shell-header' },
      createElement('h1', { className: 'shell-title' }, messages['app.title']),
      createElement('p', { className: 'shell-tagline' }, messages['app.tagline']),
    ),
    toolbar(state, toolbarCallbacks),
    createElement('main', { className: 'studio-main' }, ...mainContent),
    footer(state),
  );
}

/**
 * Full editor for a ready session: side panels, canvas, and inspector
 * composed exactly as shipped. In live mode the editing chrome is absent
 * and `liveContent` (the surface view) fills the main area instead — per
 * DESIGN_SYSTEM.md, "Surface: controls dominate". This is the single entry
 * point App renders and the accessibility contract executes — there is no
 * second markup path.
 */
export function renderStudio(
  state: EditorState,
  callbacks: StudioCallbacks,
  liveContent: ReactElement | null = null,
): ReactElement {
  const messages = messagesFor(state.locale);

  if (state.phase !== 'ready') {
    const content =
      state.phase === 'loading'
        ? createElement(
            'p',
            { key: 'loading', role: 'status', className: 'muted' },
            messages['studio.loading'],
          )
        : createElement(
            'p',
            { key: 'failed', role: 'alert', className: 'field-error' },
            formatMessage(messages['studio.loadFailed'], {
              token: state.loadErrorToken ?? '',
            }),
          );
    return renderStudioShell(state, [content], callbacks);
  }

  if (state.mode === 'live' && liveContent !== null) {
    return renderStudioShell(state, [liveContent], callbacks);
  }

  const selectedDeckId =
    state.selection?.kind === 'deck'
      ? state.selection.deckId
      : (state.snapshot.decks.find((document) =>
          document.deck.pages.some((page) => page.id === state.currentPageId),
        )?.deck.id ?? null);

  const mainContent: ReactElement[] = [
    createElement(
      'aside',
      { key: 'panels', className: 'side-column' },
      renderPagesRail(
        {
          messages,
          snapshot: state.snapshot,
          currentPageId: state.currentPageId,
          selectedDeckId,
        },
        callbacks,
      ),
      renderFoldersPanel(
        { messages, snapshot: state.snapshot, selectedDeckId },
        callbacks,
      ),
      renderProfilesPanel(
        {
          messages,
          snapshot: state.snapshot,
          selectedProfileId:
            state.selection?.kind === 'profile' ? state.selection.profileId : null,
        },
        callbacks,
      ),
    ),
    createElement(
      'div',
      { key: 'canvas-wrap', className: 'canvas-wrap' },
      renderCanvas(
        {
          messages,
          snapshot: state.snapshot,
          currentPageId: state.currentPageId,
          selectedControlId:
            state.selection?.kind === 'control' ? state.selection.controlId : null,
          lift: state.lift,
          zoomIndex: state.zoomIndex,
          diagnostics: state.diagnostics,
        },
        callbacks,
      ),
    ),
    renderInspector(
      {
        messages,
        snapshot: state.snapshot,
        selection: state.selection,
        errorText:
          state.lastOpError === null
            ? ''
            : describeOpError(state.lastOpError, state.locale),
      },
      callbacks,
    ),
  ];

  return renderStudioShell(state, mainContent, callbacks);
}
