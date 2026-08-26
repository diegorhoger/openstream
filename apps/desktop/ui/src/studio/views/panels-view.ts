/**
 * Side-panel views for the Studio editor (issue #17): pages rail, decks and
 * folders tree, and profiles.
 *
 * Every drag-style arrangement operation ships an explicit button
 * alternative right beside it: page reordering, profile deck ordering, and
 * folder moves are all reachable with Tab + Enter alone, satisfying
 * "drag/drop always has keyboard alternatives" without hidden gestures.
 */

import { createElement } from 'react';
import type { ChangeEvent, MouseEvent, ReactElement } from 'react';
import { formatMessage, type MessageCatalog } from '../../i18n/catalog.ts';
import { appIdentityErrors, hotkeyComboErrors } from '../decode.ts';
import type { DeckDocument, SwitchRule, WorkspaceSnapshot } from '../types.ts';

export interface PagesRailCallbacks {
  onPageSelect(deckId: string, pageId: string): void;
  onPageAdd(deckId: string): void;
  onPageMove(deckId: string, pageId: string, toIndex: number): void;
  onPageDelete(deckId: string, pageId: string): void;
}

export interface FoldersPanelCallbacks {
  onDeckSelect(deckId: string): void;
  onDeckFolderChange(deckId: string, folderPath: string): void;
  onDeckDelete(deckId: string): void;
}

export interface ProfilesPanelCallbacks {
  onProfileSelect(profileId: string): void;
  onProfileAddDeck(profileId: string, deckId: string): void;
  onProfileMoveDeck(profileId: string, deckId: string, toIndex: number): void;
  onProfileRemoveDeck(profileId: string, deckId: string): void;
  onProfileDelete(profileId: string): void;
  /** Binds a new explicit switch trigger to the profile (issue #19). */
  onRuleAdd(profileId: string, triggerKind: 'hotkey' | 'app_focus', triggerValue: string): void;
  /** Removes one switch rule. */
  onRuleRemove(profileId: string, ruleId: string): void;
  /** Enables or disables one switch rule without deleting it. */
  onRuleToggle(profileId: string, ruleId: string, enabled: boolean): void;
}

function heading(headingId: string, className: string, text: string): ReactElement {
  return createElement('h2', { id: headingId, className }, text);
}

/** Pages of the open deck in ordinal order plus their resolved index. */
function orderedPages(deck: DeckDocument): Array<{ id: string; index: number }> {
  return [...deck.deck.pages]
    .sort((a, b) => a.ordinal - b.ordinal)
    .map((page, position) => ({ id: page.id, index: position + 1 }));
}

/**
 * Pages rail for the open deck. Rows expose select plus explicit
 * move-up/move-down/delete buttons (the keyboard alternative to dragging a
 * page tab), each labeled with its resolved position for screen readers.
 */
export function renderPagesRail(
  props: {
    messages: MessageCatalog;
    snapshot: WorkspaceSnapshot;
    currentPageId: string | null;
    selectedDeckId: string | null;
  },
  callbacks: PagesRailCallbacks,
): ReactElement {
  const { messages, snapshot, currentPageId, selectedDeckId } = props;
  const headingId = 'pages-heading';
  const deck =
    snapshot.decks.find((document) => document.deck.id === selectedDeckId) ??
    snapshot.decks.find((document) =>
      document.deck.pages.some((page) => page.id === currentPageId),
    );

  if (deck === undefined) {
    return createElement(
      'nav',
      { className: 'panel side-panel', 'aria-labelledby': headingId },
      heading(headingId, 'panel-title', messages['studio.pages.heading']),
      createElement('p', { className: 'muted' }, messages['studio.inspector.nothingSelected']),
    );
  }

  const pages = orderedPages(deck);
  const rows = pages.map(({ id, index }) => {
    const moveUp =
      index > 1
        ? createElement(
            'button',
            {
              key: `up-${id}`,
              type: 'button',
              className: 'icon-button',
              'aria-label': formatMessage(messages['studio.pages.moveUp'], { index }),
              onClick: () => callbacks.onPageMove(deck.deck.id, id, index - 2),
            },
            '↑',
          )
        : null;
    const moveDown =
      index < pages.length
        ? createElement(
            'button',
            {
              key: `down-${id}`,
              type: 'button',
              className: 'icon-button',
              'aria-label': formatMessage(messages['studio.pages.moveDown'], { index }),
              onClick: () => callbacks.onPageMove(deck.deck.id, id, index),
            },
            '↓',
          )
        : null;
    return createElement(
      'li',
      { key: id, className: 'pages-row' },
      createElement(
        'button',
        {
          type: 'button',
          className: 'control-button',
          'aria-current': currentPageId === id ? 'true' : undefined,
          onClick: () => callbacks.onPageSelect(deck.deck.id, id),
        },
        formatMessage(messages['studio.canvas.openPage'], { index }),
      ),
      moveUp,
      moveDown,
      createElement(
        'button',
        {
          type: 'button',
          className: 'icon-button icon-button-danger',
          'aria-label': formatMessage(messages['studio.pages.delete'], { index }),
          onClick: () => callbacks.onPageDelete(deck.deck.id, id),
        },
        '✕',
      ),
    );
  });

  rows.push(
    createElement(
      'li',
      { key: 'add-page', className: 'pages-row' },
      createElement(
        'button',
        {
          type: 'button',
          className: 'control-button control-button-secondary',
          onClick: () => callbacks.onPageAdd(deck.deck.id),
        },
        messages['studio.pages.add'],
      ),
    ),
  );

  return createElement(
    'nav',
    { className: 'panel side-panel', 'aria-labelledby': headingId },
    heading(headingId, 'panel-title', messages['studio.pages.heading']),
    createElement('ul', { className: 'pages-list' }, ...rows),
  );
}

/** Distinct folder paths across live decks, sorted, root first. */
function distinctFolders(snapshot: WorkspaceSnapshot): string[] {
  const paths = new Set<string>();
  for (const document of snapshot.decks) {
    paths.add(document.deck.folder_path);
  }
  return [...paths].sort();
}

/**
 * Decks grouped by folder path. Each row offers selection, deletion, and a
 * combobox listing every existing folder plus the workspace root — the
 * keyboard alternative for dragging a deck between folders.
 */
export function renderFoldersPanel(
  props: {
    messages: MessageCatalog;
    snapshot: WorkspaceSnapshot;
    selectedDeckId: string | null;
  },
  callbacks: FoldersPanelCallbacks,
): ReactElement {
  const { messages, snapshot, selectedDeckId } = props;
  const headingId = 'folders-heading';
  const folders = distinctFolders(snapshot);

  const groups = folders.map((folder) => {
    const decksInFolder = snapshot.decks
      .filter((document) => document.deck.folder_path === folder)
      .sort((a, b) => a.deck.title.localeCompare(b.deck.title));
    const folderName =
      folder.length === 0 ? messages['studio.folders.root'] : folder;
    const rows = decksInFolder.map((document) => {
      const deckId = document.deck.id;
      const title = document.deck.title;
      const folderOptions = distinctFolders(snapshot);
      if (!folderOptions.includes(folder)) {
        folderOptions.push(folder);
      }
      return createElement(
        'li',
        { key: deckId, className: 'folder-deck-row' },
        createElement(
          'button',
          {
            type: 'button',
            className: 'control-button',
            'aria-current': selectedDeckId === deckId ? 'true' : undefined,
            'aria-label': formatMessage(messages['studio.deck.select'], { title }),
            onClick: () => callbacks.onDeckSelect(deckId),
          },
          title,
        ),
        createElement('select', {
          className: 'folder-select',
          'aria-label': formatMessage(messages['studio.deck.moveToFolder'], { title }),
          value: folder,
          onChange: (event: ChangeEvent<HTMLSelectElement>) =>
            callbacks.onDeckFolderChange(deckId, event.currentTarget.value),
        }, ...folderOptions
          .map((path) =>
            createElement(
              'option',
              { key: path || '(root)', value: path },
              path.length === 0 ? messages['studio.folders.root'] : path,
            ),
          ),
        ),
        createElement('button', {
          type: 'button',
          className: 'icon-button icon-button-danger',
          'aria-label': formatMessage(messages['studio.deck.delete'], { title }),
          onClick: () => callbacks.onDeckDelete(deckId),
        }, '✕'),
      );
    });
    return createElement(
      'li',
      { key: folder || '(root)', className: 'folder-group' },
      createElement('h3', { className: 'folder-name' }, folderName),
      createElement('ul', { className: 'folder-deck-list' }, ...rows),
    );
  });

  return createElement(
    'nav',
    { className: 'panel side-panel', 'aria-labelledby': headingId },
    heading(headingId, 'panel-title', messages['studio.folders.heading']),
    createElement('ul', { className: 'folder-groups' }, ...groups),
  );
}

/** Accessible text for one rule trigger (canonical wire form). */
function triggerText(rule: SwitchRule): string {
  return rule.trigger.kind === 'hotkey'
    ? `hotkey:${rule.trigger.combo}`
    : `app_focus:${rule.trigger.app}`;
}

/**
 * Switch-rules editor for one selected profile (issue #19): lists every
 * explicit trigger bound to this profile with enable/disable and remove
 * controls, plus a fail-closed add form. Client validation mirrors the
 * domain grammar; the Rust op re-validates authoritatively.
 */
function renderRuleEditor(
  messages: MessageCatalog,
  profileId: string,
  rules: SwitchRule[],
  callbacks: ProfilesPanelCallbacks,
): ReactElement {
  const kindId = `rule-kind-${profileId}`;
  const valueId = `rule-value-${profileId}`;

  const rows = rules.map((rule) => {
    const trigger = triggerText(rule);
    return createElement(
      'li',
      { key: rule.id, className: 'profile-rule-row' },
      createElement(
        'span',
        { className: 'profile-rule-trigger' },
        trigger,
        rule.enabled
          ? null
          : createElement(
              'span',
              { className: 'badge-latched' },
              messages['studio.profiles.rules.disabledBadge'],
            ),
      ),
      createElement('button', {
        type: 'button',
        className: 'control-button',
        onClick: () => callbacks.onRuleToggle(profileId, rule.id, !rule.enabled),
      }, formatMessage(messages[rule.enabled ? 'studio.profiles.rules.disable' : 'studio.profiles.rules.enable'], { trigger })),
      createElement('button', {
        type: 'button',
        className: 'icon-button icon-button-danger',
        'aria-label': formatMessage(messages['studio.profiles.rules.remove'], { trigger }),
        onClick: () => callbacks.onRuleRemove(profileId, rule.id),
      }, '✕'),
    );
  });

  const addClicked = (event: MouseEvent<HTMLButtonElement>): void => {
    const group = event.currentTarget.parentElement;
    if (group === null) {
      return;
    }
    const kindSelect = group.querySelector<HTMLSelectElement>(`#${CSS.escape(kindId)}`);
    const valueInput = group.querySelector<HTMLInputElement>(`#${CSS.escape(valueId)}`);
    if (kindSelect === null || valueInput === null) {
      return;
    }
    const kind = kindSelect.value === 'app_focus' ? ('app_focus' as const) : ('hotkey' as const);
    const value = valueInput.value.trim();
    // Client-side grammar mirror; the Rust op revalidates authoritatively.
    const clientErrors = kind === 'hotkey' ? hotkeyComboErrors(value) : appIdentityErrors(value);
    if (value.length === 0 || clientErrors.length > 0) {
      valueInput.setAttribute('aria-invalid', 'true');
      return;
    }
    valueInput.removeAttribute('aria-invalid');
    callbacks.onRuleAdd(profileId, kind, value);
    valueInput.value = '';
  };

  return createElement(
    'div',
    { className: 'profile-rules' },
    createElement('h3', { className: 'panel-subtitle' }, messages['studio.profiles.rules.heading']),
    rules.length === 0
      ? createElement('p', { className: 'muted' }, messages['studio.profiles.rules.none'])
      : createElement('ul', { className: 'profile-rule-list' }, ...rows),
    createElement(
      'div',
      { role: 'group', 'aria-label': messages['studio.profiles.rules.addHeading'] },
      createElement(
        'select',
        {
          id: kindId,
          className: 'folder-select',
          'aria-label': messages['studio.profiles.rules.kind'],
        },
        createElement('option', { value: 'hotkey' }, messages['studio.profiles.rules.kind.hotkey']),
        createElement(
          'option',
          { value: 'app_focus' },
          messages['studio.profiles.rules.kind.app_focus'],
        ),
      ),
      createElement('input', {
        id: valueId,
        type: 'text',
        className: 'inspector-input',
        placeholder: messages['studio.profiles.rules.value.hotkey'],
      }),
      createElement(
        'button',
        { type: 'button', className: 'control-button', onClick: addClicked },
        messages['studio.profiles.rules.add'],
      ),
      createElement(
        'p',
        { className: 'field-hint' },
        `${messages['studio.profiles.rules.value.hotkey']} · ${messages['studio.profiles.rules.value.app_focus']}`,
      ),
    ),
  );
}

/**
 * Profiles panel: profile selection, the profile's ordered deck list with
 * up/down/remove buttons per row (keyboard alternative to drag reordering),
 * a combobox to append any deck not yet referenced, and the issue #19
 * switch-rule editor for the selected profile.
 */
export function renderProfilesPanel(
  props: {
    messages: MessageCatalog;
    snapshot: WorkspaceSnapshot;
    selectedProfileId: string | null;
  },
  callbacks: ProfilesPanelCallbacks,
): ReactElement {
  const { messages, snapshot, selectedProfileId } = props;
  const headingId = 'profiles-heading';

  const selected =
    snapshot.profiles.find((document) => document.profile.id === selectedProfileId) ?? null;

  const children: ReactElement[] = [];
  for (const document of snapshot.profiles) {
    const profileId = document.profile.id;
    const name = document.profile.name;
    const isSelected = profileId === selectedProfileId;
    children.push(
      createElement(
        'div',
        { key: profileId, className: 'profile-block' },
        createElement(
          'button',
          {
            type: 'button',
            className: 'control-button',
            'aria-current': isSelected ? 'true' : undefined,
            onClick: () => callbacks.onProfileSelect(profileId),
          },
          name,
        ),
        isSelected
          ? createElement(
              'div',
              { className: 'profile-detail' },
              createElement(
                'ol',
                { className: 'profile-deck-list' },
                ...document.profile.deck_ids.map((deckId, index) => {
                  const deckTitle =
                    snapshot.decks.find((candidate) => candidate.deck.id === deckId)?.deck.title ??
                    deckId;
                  return createElement(
                    'li',
                    { key: deckId, className: 'profile-deck-row' },
                    createElement('span', { className: 'profile-deck-name' }, deckTitle),
                    index > 0
                      ? createElement('button', {
                          type: 'button',
                          className: 'icon-button',
                          'aria-label': formatMessage(messages['studio.profiles.moveUp'], {
                            deck: deckTitle,
                            name,
                          }),
                          onClick: () => callbacks.onProfileMoveDeck(profileId, deckId, index - 1),
                        }, '↑')
                      : null,
                    index < document.profile.deck_ids.length - 1
                      ? createElement('button', {
                          type: 'button',
                          className: 'icon-button',
                          'aria-label': formatMessage(messages['studio.profiles.moveDown'], {
                            deck: deckTitle,
                            name,
                          }),
                          onClick: () => callbacks.onProfileMoveDeck(profileId, deckId, index + 1),
                        }, '↓')
                      : null,
                    createElement('button', {
                      type: 'button',
                      className: 'icon-button icon-button-danger',
                      'aria-label': formatMessage(messages['studio.profiles.removeDeck'], {
                        deck: deckTitle,
                        name,
                      }),
                      onClick: () => callbacks.onProfileRemoveDeck(profileId, deckId),
                    }, '✕'),
                  );
                }),
              ),
              createElement('select', {
                className: 'folder-select',
                'aria-label': formatMessage(messages['studio.profiles.addDeck'], { name }),
                value: '',
                onChange: (event: ChangeEvent<HTMLSelectElement>) => {
                  const deckId = event.currentTarget.value;
                  if (deckId.length > 0) {
                    callbacks.onProfileAddDeck(profileId, deckId);
                  }
                },
              },
              createElement('option', { value: '' }, messages['studio.profiles.addDeck']),
              ...snapshot.decks
                .filter((candidate) => !document.profile.deck_ids.includes(candidate.deck.id))
                .map((candidate) =>
                  createElement(
                    'option',
                    { key: candidate.deck.id, value: candidate.deck.id },
                    candidate.deck.title,
                  ),
                ),
              ),
              createElement('button', {
                type: 'button',
                className: 'control-button control-button-danger',
                onClick: () => callbacks.onProfileDelete(profileId),
              }, messages['studio.profile.delete']),
              renderRuleEditor(
                messages,
                profileId,
                document.profile.switch_rules ?? [],
                callbacks,
              ),
            )
          : null,
      ),
    );
  }

  if (snapshot.profiles.length === 0 && selected === null) {
    children.push(
      createElement('p', { key: 'empty', className: 'muted' }, messages['studio.profiles.none']),
    );
  }

  return createElement(
    'nav',
    { className: 'panel side-panel', 'aria-labelledby': headingId },
    heading(headingId, 'panel-title', messages['studio.profiles.heading']),
    ...children,
  );
}
