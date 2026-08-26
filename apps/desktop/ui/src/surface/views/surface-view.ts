/**
 * Live deck surface view (issue #18).
 *
 * Plain element factories so the exact shipped markup is executable by
 * both Vite and the Node accessibility-contract suite — there is no second
 * markup path. Accessibility invariants encoded here:
 *
 * - one labelled section with an h2; page tabs are real buttons with
 *   aria-current;
 * - every deck key carries its label plus kind and execution phase IN TEXT
 *   (badge + accessible-name suffix) and an aria-hidden glyph — status is
 *   never color alone (DESIGN_SYSTEM.md);
 * - relayed, accepted, running, and each terminal render as DISTINCT text
 *   badges with their own data-phase hook for color; nothing renders
 *   success unless the machine entered it authoritatively;
 * - armed keys announce through role="alert" and resolve only via the
 *   confirm/cancel group (no timeout, per the a11y contract);
 * - state changes flow through one polite live region; refusals/failures
 *   additionally use role="alert";
 * - engine availability is always textual next to the decorative dot;
 * - grid geometry is stated in words in the group's accessible name.
 */

import { createElement } from 'react';
import type { KeyboardEvent, PointerEvent, ReactElement } from 'react';
import { formatMessage, type MessageCatalog } from '../../i18n/catalog.ts';
import { findPage } from '../../studio/editor.ts';
import { TARGETS } from '../../design/tokens.ts';
import type { Control, Page, WorkspaceSnapshot } from '../../studio/types.ts';
import {
  INITIAL_KEY_RUNTIME,
  HOLD_THRESHOLD_MS,
  REPEAT_INTERVAL_MS,
  type ExecutionPhase,
  type InteractionContext,
  type KeyRuntime,
} from '../machine.ts';

const CELL_GAP_PX = 8;

/** Decorative glyphs paired with mandatory text badges (never color alone). */
export const PHASE_GLYPHS: Readonly<Record<ExecutionPhase, string>> = {
  idle: '',
  pressed: '●',
  armed: '⚠',
  relayed: '»',
  accepted: '·',
  running: '∘',
  succeeded: '✓',
  failed: '✕',
  cancelled: '⊘',
  expired: '!',
  outcome_unknown: '?',
};

function phaseBadgeText(messages: MessageCatalog, phase: ExecutionPhase): string {
  const key = `surface.phase.${phase}` as keyof MessageCatalog;
  return messages[key] ?? phase;
}

/** Accessible name for one deck key: label, kind, then state in words. */
export function keyAccessibleName(
  messages: MessageCatalog,
  control: Control,
  runtime: KeyRuntime,
): string {
  const kindKey = `studio.control.kind.${control.kind}` as keyof MessageCatalog;
  const parts = [control.label, messages[kindKey]];
  if (!control.enabled) {
    parts.push(messages['studio.state.disabled']);
  }
  if (runtime.latched) {
    parts.push(messages['surface.phase.latched']);
  }
  if (runtime.phase !== 'idle') {
    parts.push(phaseBadgeText(messages, runtime.phase));
  }
  return parts.join(', ');
}

/**
 * Localized sentence describing a phase of one key — used for the polite
 * live region and failure alerts.
 */
export function phaseAnnouncement(
  messages: MessageCatalog,
  label: string,
  phase: ExecutionPhase,
  failureToken: string | null,
): string {
  switch (phase) {
    case 'pressed':
      return formatMessage(messages['surface.announce.pressed'], { name: label });
    case 'armed':
      return formatMessage(messages['surface.announce.armed'], { name: label });
    case 'relayed':
      return formatMessage(messages['surface.announce.relayed'], { name: label });
    case 'accepted':
      return formatMessage(messages['surface.announce.accepted'], { name: label });
    case 'running':
      return formatMessage(messages['surface.announce.running'], { name: label });
    case 'succeeded':
      return formatMessage(messages['surface.announce.succeeded'], { name: label });
    case 'failed':
      return formatMessage(
        messages['surface.announce.failed'],
        { name: label, token: failureToken ?? 'unknown' },
      );
    case 'cancelled':
      return formatMessage(messages['surface.announce.cancelled'], { name: label });
    case 'expired':
      return formatMessage(messages['surface.announce.expired'], { name: label });
    case 'outcome_unknown':
      return formatMessage(messages['surface.announce.outcomeUnknown'], { name: label });
    case 'idle':
    default:
      return '';
  }
}

/** Localized sentence for one refusal token (closed vocabulary). */
export function describeRefusal(messages: MessageCatalog, token: string): string {
  const directKeys = [
    'binding_absent',
    'control_disabled',
    'state_sink_no_interaction',
  ] as const;
  if ((directKeys as readonly string[]).includes(token)) {
    const key = `surface.error.${token}` as keyof MessageCatalog;
    return messages[key];
  }
  if (token.startsWith('policy_mismatch:')) {
    return formatMessage(messages['surface.error.policy_mismatch'], {
      event: token.slice('policy_mismatch:'.length),
    });
  }
  if (token.startsWith('not_found:') || token.startsWith('invalid_id:')) {
    return messages['studio.error.not_found'];
  }
  return messages['surface.error.unknown'];
}

export interface SurfaceProps {
  readonly messages: MessageCatalog;
  readonly snapshot: WorkspaceSnapshot;
  readonly currentPageId: string | null;
  readonly engineAvailable: boolean;
  /** Runtime by control id (missing entries read as idle). */
  readonly runtimes: Readonly<Record<string, KeyRuntime>>;
  /** Ids of currently armed controls (arming strip renders when any). */
  readonly armedControlIds: readonly string[];
  /** Latest announcement sentence (polite live region). */
  readonly announcement: string;
  /** Latest failure/refusal sentence (role="alert"). */
  readonly alert: string;
  /** Sequence bump so identical consecutive announcements re-fire. */
  readonly announcementSeq: number;
}

export interface SurfaceCallbacks {
  onPageSelect(pageId: string): void;
  onPressBegin(controlId: string): void;
  onPressEnd(controlId: string): void;
  onArmConfirm(controlId: string): void;
  onArmCancel(controlId: string): void;
  onSurfaceKeyDown(
    controlId: string,
    context: InteractionContext,
    event: KeyboardEvent<HTMLButtonElement>,
  ): void;
}

function contextOf(control: Control): InteractionContext & { destructive: boolean } {
  // No binding vocabulary exists this milestone, so NO production control
  // is destructive-class yet; the arming gate stays proven by tests.
  return {
    enabled: control.enabled,
    policy: control.policy,
    destructive: false,
  };
}

function deckKey(
  props: SurfaceProps,
  control: Control,
  stridePx: number,
  callbacks: SurfaceCallbacks,
): ReactElement {
  const { messages, runtimes } = props;
  const runtime = runtimes[control.id] ?? INITIAL_KEY_RUNTIME;
  const describedBy =
    !control.enabled || control.kind === 'variable_display'
      ? `key-desc-${control.id}`
      : undefined;
  return createElement(
    'button',
    {
      key: control.id,
      type: 'button',
      className: `deck-key surface-key deck-key-kind-${control.kind}`,
      'data-phase': runtime.phase,
      style: {
        left: `${control.geometry.x * stridePx}px`,
        top: `${control.geometry.y * stridePx}px`,
        width: `${control.geometry.width * stridePx - CELL_GAP_PX}px`,
        height: `${control.geometry.height * stridePx - CELL_GAP_PX}px`,
      },
      disabled: !control.enabled || control.kind === 'variable_display',
      'aria-label': keyAccessibleName(messages, control, runtime),
      'aria-describedby': describedBy,
      'aria-pressed':
        control.kind === 'toggle' && control.enabled ? runtime.latched : undefined,
      onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
        if (event.button !== 0) {
          return;
        }
        event.preventDefault();
        callbacks.onPressBegin(control.id);
      },
      onPointerUp: (event: PointerEvent<HTMLButtonElement>) => {
        if (event.button !== 0) {
          return;
        }
        event.preventDefault();
        callbacks.onPressEnd(control.id);
      },
      onPointerLeave: () => {
        // Sliding off the key releases it honestly instead of leaving a
        // stuck press.
        callbacks.onPressEnd(control.id);
      },
      onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => {
        callbacks.onSurfaceKeyDown(control.id, contextOf(control), event);
      },
    },
    createElement(
      'span',
      { className: 'deck-key-label' },
      createElement(
        'span',
        { 'aria-hidden': 'true', className: `key-glyph phase-${runtime.phase}` },
        PHASE_GLYPHS[runtime.phase],
      ),
      control.label,
    ),
    runtime.phase !== 'idle'
      ? createElement(
          'span',
          { className: `deck-key-badge badge-phase phase-${runtime.phase}` },
          phaseBadgeText(messages, runtime.phase),
        )
      : null,
    runtime.latched && control.kind === 'toggle'
      ? createElement(
          'span',
          { className: 'deck-key-badge badge-latched' },
          messages['surface.phase.latched'],
        )
      : null,
    describedBy !== undefined
      ? createElement(
          'span',
          { id: describedBy, className: 'visually-hidden' },
          control.kind === 'variable_display'
            ? messages['surface.key.stateSink']
            : messages['studio.state.disabled'],
        )
      : null,
  );
}

/**
 * Renders the complete live surface section for the current page.
 */
export function renderSurface(props: SurfaceProps, callbacks: SurfaceCallbacks): ReactElement {
  const { messages, snapshot, currentPageId, engineAvailable, armedControlIds } = props;
  const headingId = 'surface-heading';

  const engineLine = createElement(
    'p',
    { key: 'engine', className: 'status-line', role: 'status' },
    createElement('span', { 'aria-hidden': 'true', className: 'status-dot' }),
    createElement('span', { className: 'visually-hidden' }, messages['engine.status.label']),
    engineAvailable ? messages['surface.engine.ready'] : messages['surface.engine.unavailable'],
  );

  const deck = snapshot.decks.find((document) =>
    document.deck.pages.some((page) => page.id === currentPageId),
  );
  const page: Page | null = findPage(snapshot, currentPageId);

  if (deck === undefined || page === null) {
    return createElement(
      'section',
      { className: 'panel surface-panel', 'aria-labelledby': headingId },
      createElement(
        'h2',
        { id: headingId, className: 'panel-title' },
        messages['surface.heading'],
      ),
      engineLine,
      createElement('p', { className: 'muted' }, messages['surface.empty']),
    );
  }

  const sortedPages = [...deck.deck.pages].sort((a, b) => a.ordinal - b.ordinal);
  const pageIndex = sortedPages.findIndex((candidate) => candidate.id === page.id) + 1;
  const cell = TARGETS.deckKeyWidthPx;
  const stride = cell + CELL_GAP_PX;

  const tabs = createElement(
    'nav',
    { key: 'tabs', className: 'page-tabs', 'aria-label': messages['surface.pages.label'] },
    ...sortedPages.map((candidate) =>
      createElement(
        'button',
        {
          key: candidate.id,
          type: 'button',
          className:
            candidate.id === page.id ? 'control-button page-tab-current' : 'control-button',
          'aria-current': candidate.id === page.id ? 'true' : undefined,
          onClick: () => callbacks.onPageSelect(candidate.id),
        },
        formatMessage(messages['surface.pages.tab'], { index: candidate.ordinal + 1 }),
      ),
    ),
  );

  const grid = createElement(
    'div',
    {
      key: 'grid',
      role: 'group',
      className: 'canvas-grid surface-grid',
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
    },
    ...page.controls.map((control) => deckKey(props, control, stride, callbacks)),
  );

  // Destructive arming resolves ONLY through this group — no timeout, no
  // dismissal by pressing elsewhere (a11y contract: no timeout-dependent
  // interaction without extension).
  const armedControl = armedControlIds
    .map((id) => page.controls.find((control) => control.id === id))
    .find((control): control is Control => control !== undefined);
  const armingStrip =
    armedControl === undefined
      ? null
      : createElement(
          'div',
          {
            key: 'arming',
            role: 'group',
            className: 'arming-strip',
            'aria-label': messages['surface.arming.group'],
          },
          createElement(
            'p',
            { role: 'alert', className: 'field-error' },
            formatMessage(messages['surface.arming.title'], { name: armedControl.label }),
          ),
          createElement(
            'button',
            {
              type: 'button',
              className: 'control-button control-button-danger',
              onClick: () => callbacks.onArmConfirm(armedControl.id),
            },
            formatMessage(messages['surface.arming.confirm'], { name: armedControl.label }),
          ),
          createElement(
            'button',
            {
              type: 'button',
              className: 'control-button',
              onClick: () => callbacks.onArmCancel(armedControl.id),
            },
            formatMessage(messages['surface.arming.cancel'], { name: armedControl.label }),
          ),
        );

  // Zero-width-space alternation re-speaks identical consecutive texts.
  const announcement =
    props.announcement.length === 0
      ? ''
      : props.announcementSeq % 2 === 0
        ? props.announcement
        : `${props.announcement}\u200b`;

  const tail: ReactElement[] = [
    createElement(
      'p',
      {
        key: 'live',
        className: 'visually-hidden',
        'aria-live': 'polite' as const,
      },
      announcement,
    ),
  ];
  if (props.alert.length > 0) {
    tail.push(
      createElement(
        'p',
        { key: 'alert', role: 'alert', className: 'field-error surface-alert' },
        props.alert,
      ),
    );
  }
  tail.push(
    createElement(
      'p',
      { key: 'hint', className: 'field-hint' },
      formatMessage(messages['surface.hint'], {
        hold: HOLD_THRESHOLD_MS,
        repeat: REPEAT_INTERVAL_MS,
      }),
    ),
  );

  return createElement(
    'section',
    { className: 'panel surface-panel canvas-panel', 'aria-labelledby': headingId },
    createElement(
      'h2',
      { id: headingId, className: 'panel-title' },
      messages['surface.heading'],
    ),
    engineLine,
    tabs,
    grid,
    armingStrip,
    ...tail,
  );
}
