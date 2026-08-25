/**
 * Inspector view (issue #17): property editing for the current selection.
 *
 * Validation feedback comes from the authoritative Rust service: refused
 * ops arrive as closed-vocabulary tokens which App localizes into
 * `errorText`; the inspector renders it as a role="alert" line so screen
 * readers announce it immediately. Field values are uncontrolled and commit
 * on blur or Enter, so typing never spams one undo entry per keystroke.
 */

import { createElement } from 'react';
import type { KeyboardEvent, ReactElement } from 'react';
import { formatMessage, type MessageCatalog } from '../../i18n/catalog.ts';
import type {
  ControlKind,
  DeckDocument,
  InteractionPolicy,
  Page,
  ProfileDocument,
  WorkspaceSnapshot,
} from '../types.ts';
import { CONTROL_KINDS, KIND_ALLOWS_POLICY } from '../types.ts';

export interface InspectorCallbacks {
  onControlLabel(controlId: string, label: string): void;
  onControlKind(controlId: string, kind: ControlKind): void;
  onControlPolicy(controlId: string, policy: InteractionPolicy | null): void;
  onControlEnabled(controlId: string, enabled: boolean): void;
  onControlGeometryPatch(
    controlId: string,
    patch: { x?: number; y?: number; width?: number; height?: number },
  ): void;
  onControlDelete(controlId: string): void;
  onPageGrid(deckId: string, pageId: string, columns: number, rows: number): void;
  onPageDelete(deckId: string, pageId: string): void;
  onDeckTitle(deckId: string, title: string): void;
  onDeckFolder(deckId: string, folderPath: string): void;
  onDeckDelete(deckId: string): void;
  onProfileName(profileId: string, name: string): void;
}

export interface InspectorProps {
  readonly messages: MessageCatalog;
  readonly snapshot: WorkspaceSnapshot;
  readonly selection:
    | { kind: 'control'; pageId: string; controlId: string }
    | { kind: 'page'; pageId: string }
    | { kind: 'deck'; deckId: string }
    | { kind: 'profile'; profileId: string }
    | null;
  /** Localized text of the last refusal (empty when none). */
  readonly errorText: string;
}

function numberField(
  keyBase: string,
  labelId: string,
  labelText: string,
  value: number,
  min: number,
  commit: (value: number) => void,
): ReactElement {
  return createElement(
    'div',
    { key: keyBase, className: 'field' },
    createElement('label', { htmlFor: labelId }, labelText),
    createElement('input', {
      id: labelId,
      key: `${labelId}-${value}`,
      className: 'field-input',
      type: 'number',
      inputMode: 'numeric',
      min,
      defaultValue: value,
      onBlur: (event: { currentTarget: HTMLInputElement }) => {
        const parsed = Number.parseInt(event.currentTarget.value, 10);
        if (Number.isInteger(parsed) && parsed >= min) {
          commit(parsed);
        }
      },
      onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === 'Enter') {
          const parsed = Number.parseInt(event.currentTarget.value, 10);
          if (Number.isInteger(parsed) && parsed >= min) {
            commit(parsed);
          }
        }
      },
    }),
  );
}

function textField(
  keyBase: string,
  labelId: string,
  labelText: string,
  value: string,
  commit: (value: string) => void,
): ReactElement {
  return createElement(
    'div',
    { key: keyBase, className: 'field' },
    createElement('label', { htmlFor: labelId }, labelText),
    createElement('input', {
      id: labelId,
      key: `${labelId}-${value.length}`,
      className: 'field-input',
      type: 'text',
      defaultValue: value,
      onKeyDown: (event: KeyboardEvent<HTMLInputElement>) =>
        event.key === 'Enter'
          ? commit(event.currentTarget.value)
          : undefined,
      onBlur: (event: { currentTarget: HTMLInputElement }) =>
        commit(event.currentTarget.value),
    }),
  );
}

function controlSection(
  messages: MessageCatalog,
  control: import('../types.ts').Control,
  callbacks: InspectorCallbacks,
): ReactElement {
  const id = control.id;
  const policies = KIND_ALLOWS_POLICY[control.kind];
  const policyOptions: ReactElement[] = [
    ...policies.map((policy) =>
      createElement(
        'option',
        { key: policy, value: policy },
        messages[`studio.policy.${policy}` as keyof MessageCatalog],
      ),
    ),
  ];
  if (control.kind === 'variable_display') {
    policyOptions.unshift(
      createElement('option', { key: 'none', value: '' }, messages['studio.inspector.noPolicy']),
    );
  }
  return createElement(
    'div',
    { className: 'inspector-body' },
    createElement(
      'p',
      { className: 'inspector-context' },
      formatMessage(messages['studio.inspector.selectedControl'], { label: control.label }),
    ),
    textField(
      'label',
      'inspector-control-label',
      messages['studio.inspector.labelField'],
      control.label,
      (value) => callbacks.onControlLabel(id, value),
    ),
    createElement(
      'div',
      { className: 'field' },
      createElement('label', { htmlFor: 'inspector-control-kind' }, messages['studio.inspector.kindField']),
      createElement('select', {
        id: 'inspector-control-kind',
        key: `kind-${control.kind}`,
        className: 'folder-select',
        defaultValue: control.kind,
        onBlur: (event: { currentTarget: HTMLSelectElement }) => {
          if (event.currentTarget.value !== control.kind) {
            callbacks.onControlKind(id, event.currentTarget.value as ControlKind);
          }
        },
      }, ...CONTROL_KINDS.map((kind) =>
        createElement('option', { key: kind, value: kind }, kindLabel(messages, kind)),
      )),
    ),
    createElement(
      'div',
      { className: 'field' },
      createElement('label', { htmlFor: 'inspector-control-policy' }, messages['studio.inspector.policyField']),
      createElement('select', {
        id: 'inspector-control-policy',
        key: `policy-${control.policy ?? 'none'}-${control.kind}`,
        className: 'folder-select',
        defaultValue: control.policy ?? '',
        onBlur: (event: { currentTarget: HTMLSelectElement }) => {
          const raw = event.currentTarget.value;
          const policy = raw.length === 0 ? null : (raw as InteractionPolicy);
          if (policy !== control.policy) {
            callbacks.onControlPolicy(id, policy);
          }
        },
      }, ...policyOptions),
    ),
    createElement(
      'div',
      { className: 'field field-checkbox' },
      createElement('input', {
        id: 'inspector-control-enabled',
        type: 'checkbox',
        defaultChecked: control.enabled,
        onChange: (event: { currentTarget: HTMLInputElement }) =>
          callbacks.onControlEnabled(id, event.currentTarget.checked),
      }),
      createElement('label', { htmlFor: 'inspector-control-enabled' }, messages['studio.inspector.enabledField']),
    ),
    numberField('x', 'inspector-control-x', messages['studio.inspector.xField'], control.geometry.x, 0, (v) =>
      callbacks.onControlGeometryPatch(id, { x: v }),
    ),
    numberField('y', 'inspector-control-y', messages['studio.inspector.yField'], control.geometry.y, 0, (v) =>
      callbacks.onControlGeometryPatch(id, { y: v }),
    ),
    numberField('width', 'inspector-control-width', messages['studio.inspector.widthField'], control.geometry.width, 1, (v) =>
      callbacks.onControlGeometryPatch(id, { width: v }),
    ),
    numberField('height', 'inspector-control-height', messages['studio.inspector.heightField'], control.geometry.height, 1, (v) =>
      callbacks.onControlGeometryPatch(id, { height: v }),
    ),
    createElement(
      'button',
      {
        type: 'button',
        className: 'control-button control-button-danger',
        onClick: () => callbacks.onControlDelete(id),
      },
      messages['studio.inspector.deleteControl'],
    ),
  );
}

function kindLabel(messages: MessageCatalog, kind: ControlKind): string {
  return messages[`studio.control.kind.${kind}` as keyof MessageCatalog] ?? kind;
}

function pageSection(
  messages: MessageCatalog,
  page: Page,
  deck: DeckDocument,
  callbacks: InspectorCallbacks,
): ReactElement {
  const index = [...deck.deck.pages].sort((a, b) => a.ordinal - b.ordinal).findIndex((p) => p.id === page.id) + 1;
  return createElement(
    'div',
    { className: 'inspector-body' },
    createElement(
      'p',
      { className: 'inspector-context' },
      formatMessage(messages['studio.inspector.selectedPage'], { index }),
    ),
    numberField('columns', 'inspector-page-columns', messages['studio.inspector.columnsField'], page.grid.columns, 1, (columns) =>
      callbacks.onPageGrid(deck.deck.id, page.id, columns, page.grid.rows),
    ),
    numberField('rows', 'inspector-page-rows', messages['studio.inspector.rowsField'], page.grid.rows, 1, (rows) =>
      callbacks.onPageGrid(deck.deck.id, page.id, page.grid.columns, rows),
    ),
    createElement(
      'button',
      {
        type: 'button',
        className: 'control-button control-button-danger',
        onClick: () => callbacks.onPageDelete(deck.deck.id, page.id),
      },
      messages['studio.inspector.deletePage'],
    ),
  );
}

function deckSection(
  messages: MessageCatalog,
  deck: DeckDocument,
  callbacks: InspectorCallbacks,
): ReactElement {
  return createElement(
    'div',
    { className: 'inspector-body' },
    createElement(
      'p',
      { className: 'inspector-context' },
      formatMessage(messages['studio.inspector.selectedDeck'], { title: deck.deck.title }),
    ),
    textField(
      'title',
      'inspector-deck-title',
      messages['studio.inspector.titleField'],
      deck.deck.title,
      (value) => callbacks.onDeckTitle(deck.deck.id, value),
    ),
    textField(
      'folder',
      'inspector-deck-folder',
      messages['studio.inspector.folderField'],
      deck.deck.folder_path,
      (value) => callbacks.onDeckFolder(deck.deck.id, value),
    ),
    createElement(
      'button',
      {
        type: 'button',
        className: 'control-button control-button-danger',
        onClick: () => callbacks.onDeckDelete(deck.deck.id),
      },
      messages['studio.inspector.deleteDeck'],
    ),
  );
}

function profileSection(
  messages: MessageCatalog,
  profile: ProfileDocument,
  callbacks: InspectorCallbacks,
): ReactElement {
  return createElement(
    'div',
    { className: 'inspector-body' },
    createElement(
      'p',
      { className: 'inspector-context' },
      formatMessage(messages['studio.inspector.selectedProfile'], { name: profile.profile.name }),
    ),
    textField(
      'name',
      'inspector-profile-name',
      messages['studio.inspector.nameField'],
      profile.profile.name,
      (value) => callbacks.onProfileName(profile.profile.id, value),
    ),
  );
}

/**
 * Renders the inspector section for whatever is selected. Uncontrolled
 * inputs commit on blur or Enter; App skips commits that do not change the
 * committed value so tabbing through fields never creates undo noise.
 */
export function renderInspector(props: InspectorProps, callbacks: InspectorCallbacks): ReactElement {
  const { messages, snapshot, selection, errorText } = props;
  const headingId = 'inspector-heading';

  let body: ReactElement;
  if (selection === null) {
    body = createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']);
  } else if (selection.kind === 'control') {
    let found: import('../types.ts').Control | null = null;
    for (const deck of snapshot.decks) {
      for (const page of deck.deck.pages) {
        const control = page.controls.find((candidate) => candidate.id === selection.controlId);
        if (control !== undefined) {
          found = control;
        }
      }
    }
    body =
      found !== null
        ? controlSection(messages, found, callbacks)
        : createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']);
  } else if (selection.kind === 'page') {
    const located: { page: Page; deck: DeckDocument } | null = (() => {
      for (const deck of snapshot.decks) {
        const page = deck.deck.pages.find((candidate) => candidate.id === selection.pageId);
        if (page !== undefined) {
          return { page, deck };
        }
      }
      return null;
    })();
    body =
      located !== null
        ? pageSection(messages, located.page, located.deck, callbacks)
        : createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']);
  } else if (selection.kind === 'deck') {
    const deck = snapshot.decks.find((document) => document.deck.id === selection.deckId);
    body =
      deck !== undefined
        ? deckSection(messages, deck, callbacks)
        : createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']);
  } else {
    const profile = snapshot.profiles.find(
      (document) => document.profile.id === selection.profileId,
    );
    body =
      profile !== undefined
        ? profileSection(messages, profile, callbacks)
        : createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']);
  }

  return createElement(
    'section',
    { className: 'panel side-panel inspector-panel', 'aria-labelledby': headingId },
    createElement('h2', { id: headingId, className: 'panel-title' }, messages['studio.inspector.heading']),
    body,
    errorText.length > 0
      ? createElement('p', { role: 'alert', className: 'field-error' }, errorText)
      : null,
  );
}
