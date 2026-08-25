/**
 * Deck-canvas view (issue #17): the editable page grid.
 *
 * Plain element factories so the exact shipped markup is executable by both
 * Vite and the Node accessibility-contract suite. Accessibility invariants:
 *
 * - one labelled section per canvas with an h2;
 * - the grid is role="group" whose accessible name states index, columns,
 *   and rows in words (never color or geometry alone);
 * - every control is a real <button> carrying its label plus kind and state
 *   IN TEXT ("Disabled", "lifted…"), satisfying "status is never color
 *   alone";
 * - overlapping-control warnings are text-linked via aria-describedby;
 * - the keyboard alternative is first-class: Enter lifts/drops, arrows move
 *   or resize while lifted (bindings resolved by App via handleCanvasKeys),
 *   and HTML5 drag events carry the same op-producing information.
 */

import { createElement } from 'react';
import type { DragEvent, KeyboardEvent, MouseEvent, ReactElement } from 'react';
import { formatMessage, type MessageCatalog } from '../../i18n/catalog.ts';
import { TARGETS } from '../../design/tokens.ts';
import {
  findPage,
  ZOOM_LEVELS,
  type LiftState,
} from '../editor.ts';
import {
  CONTROL_KINDS,
  type Control,
  type Diagnostic,
  type Page,
  type WorkspaceSnapshot,
} from '../types.ts';

/** Grid gap between cells, from the 8 px spacing rhythm. */
const CELL_GAP_PX = 8;

export interface CanvasProps {
  readonly messages: MessageCatalog;
  readonly snapshot: WorkspaceSnapshot;
  readonly currentPageId: string | null;
  readonly selectedControlId: string | null;
  readonly lift: LiftState | null;
  readonly zoomIndex: number;
  readonly diagnostics: readonly Diagnostic[];
}

export interface CanvasCallbacks {
  /** Selects a control (pointer click); keyboard lift flows through keys. */
  onSelectControl(pageId: string, controlId: string, label: string): void;
  /** Raw key event; App resolves bindings and calls preventDefault itself. */
  onCanvasKeyDown(event: KeyboardEvent): void;
  /** Pointer drag start; the event carries the drag payload. */
  onControlDragStart(controlId: string, event: DragEvent<HTMLButtonElement>): void;
  /** Drop anywhere on the grid; App resolves cell coordinates. */
  onGridDrop(event: DragEvent<HTMLDivElement>): void;
  /** Adds a control of `kind` at the first free cell. */
  onAddControl(kind: (typeof CONTROL_KINDS)[number]): void;
}

function kindLabel(messages: MessageCatalog, kind: Control['kind']): string {
  const key = `studio.control.kind.${kind}` as keyof MessageCatalog;
  return messages[key] ?? kind;
}

function addLabel(messages: MessageCatalog, kind: Control['kind']): string {
  switch (kind) {
    case 'button':
      return messages['studio.canvas.addButton'];
    case 'toggle':
      return messages['studio.canvas.addToggle'];
    case 'page_jump':
      return messages['studio.canvas.addPageJump'];
    case 'variable_display':
      return messages['studio.canvas.addDisplay'];
  }
}

function controlAccessibleName(
  messages: MessageCatalog,
  control: Control,
  lifted: boolean,
): string {
  const parts = [control.label, kindLabel(messages, control.kind)];
  if (!control.enabled) {
    parts.push(messages['studio.state.disabled']);
  }
  if (lifted) {
    parts.push(messages['studio.state.liftedSuffix']);
  }
  return parts.join(', ');
}

function collides(diagnostics: readonly Diagnostic[], controlId: string): boolean {
  return diagnostics.some((diagnostic) => diagnostic.control_ids.includes(controlId));
}

/**
 * Renders the canvas section for the current page, or an inert hint when no
 * page exists yet.
 */
export function renderCanvas(props: CanvasProps, callbacks: CanvasCallbacks): ReactElement {
  const { messages, snapshot, currentPageId, selectedControlId, lift, zoomIndex, diagnostics } =
    props;
  const zoom = ZOOM_LEVELS[zoomIndex] ?? 1;
  const deck = snapshot.decks.find((document) =>
    document.deck.pages.some((page) => page.id === currentPageId),
  );
  const page: Page | null = findPage(snapshot, currentPageId);
  const headingId = 'canvas-heading';

  if (deck === undefined || page === null) {
    return createElement(
      'section',
      { className: 'panel canvas-panel', 'aria-labelledby': headingId },
      createElement('h2', { id: headingId, className: 'panel-title' }, messages['studio.canvas.heading']),
      createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']),
    );
  }

  const sortedPages = [...deck.deck.pages].sort((a, b) => a.ordinal - b.ordinal);
  const pageIndex = sortedPages.findIndex((candidate) => candidate.id === page.id) + 1;
  const cell = TARGETS.deckKeyWidthPx * zoom;
  const stride = cell + CELL_GAP_PX;

  const gridChildren: ReactElement[] = [];

  if (lift !== null) {
    gridChildren.push(
      createElement('div', {
        key: 'lift-ghost',
        'aria-hidden': 'true',
        className: 'canvas-lift-ghost',
        style: {
          left: `${lift.x * stride}px`,
          top: `${lift.y * stride}px`,
          width: `${lift.width * stride - CELL_GAP_PX}px`,
          height: `${lift.height * stride - CELL_GAP_PX}px`,
        },
      }),
    );
  }

  for (const control of page.controls) {
    const liftedHere = lift !== null && lift.controlId === control.id;
    const position = liftedHere && lift !== null ? lift : control.geometry;
    const classes = [
      'deck-key',
      `deck-key-kind-${control.kind}`,
      selectedControlId === control.id ? 'deck-key-selected' : '',
      liftedHere ? 'deck-key-lifted' : '',
      control.enabled ? '' : 'deck-key-disabled',
    ].filter(Boolean);
    gridChildren.push(
      createElement(
        'button',
        {
          key: control.id,
          type: 'button',
          className: classes.join(' '),
          style: {
            left: `${position.x * stride}px`,
            top: `${position.y * stride}px`,
            width: `${position.width * stride - CELL_GAP_PX}px`,
            height: `${position.height * stride - CELL_GAP_PX}px`,
          },
          draggable: true,
          'aria-label': controlAccessibleName(messages, control, liftedHere),
          'aria-describedby': collides(diagnostics, control.id)
            ? 'collision-description'
            : undefined,
          'data-control-id': control.id,
          onClick: (event: MouseEvent<HTMLButtonElement>) => {
            event.preventDefault();
            callbacks.onSelectControl(page.id, control.id, control.label);
          },
          onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) =>
            callbacks.onCanvasKeyDown(event),
          onDragStart: (event: DragEvent<HTMLButtonElement>) =>
            callbacks.onControlDragStart(control.id, event),
        },
        createElement('span', { className: 'deck-key-label' }, control.label),
        control.enabled
          ? null
          : createElement(
              'span',
              { className: 'deck-key-badge', 'aria-hidden': 'true' },
              messages['studio.state.disabled'],
            ),
        collides(diagnostics, control.id)
          ? createElement(
              'span',
              { className: 'deck-key-badge deck-key-badge-warning', 'aria-hidden': 'true' },
              messages['studio.collision.badge'],
            )
          : null,
      ),
    );
  }

  const collisionNote =
    diagnostics.some((diagnostic) => diagnostic.page_id === page.id && diagnostic.control_ids.length > 0)
      ? createElement(
          'p',
          { id: 'collision-description', className: 'field-hint' },
          messages['studio.collision.description'],
        )
      : null;

  return createElement(
    'section',
    { className: 'panel canvas-panel', 'aria-labelledby': headingId },
    createElement(
      'h2',
      { id: headingId, className: 'panel-title' },
      messages['studio.canvas.heading'],
    ),
    createElement(
      'div',
      {
        key: 'grid',
        role: 'group',
        className: 'canvas-grid',
        'aria-label': formatMessage(messages['studio.canvas.gridLabel'], {
          index: pageIndex,
          total: sortedPages.length,
          columns: page.grid.columns,
          rows: page.grid.rows,
        }),
        'data-page-id': page.id,
        style: {
          width: `${page.grid.columns * stride - CELL_GAP_PX}px`,
          height: `${page.grid.rows * stride - CELL_GAP_PX}px`,
          backgroundSize: `${stride}px ${stride}px`,
        },
        onDragOver: (event: DragEvent<HTMLDivElement>) => event.preventDefault(),
        onDrop: (event: DragEvent<HTMLDivElement>) => {
          event.preventDefault();
          callbacks.onGridDrop(event);
        },
      },
      ...gridChildren,
    ),
    collisionNote,
    createElement(
      'div',
      {
        key: 'add-palette',
        role: 'group',
        className: 'add-palette',
        'aria-label': messages['studio.canvas.addControlHeading'],
      },
      createElement(
        'h3',
        { className: 'add-palette-title' },
        messages['studio.canvas.addControlHeading'],
      ),
      ...CONTROL_KINDS.map((kind) =>
        createElement(
          'button',
          { key: kind, type: 'button', className: 'control-button', onClick: () => callbacks.onAddControl(kind) },
          addLabel(messages, kind),
        ),
      ),
    ),
  );
}
