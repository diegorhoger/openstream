/**
 * Profile switching panel view (issue #19).
 *
 * Plain element factories like the other views so the exact shipped markup
 * is executable by both Vite and the Node accessibility-contract suite.
 * Accessibility invariants encoded here:
 *
 * - one labelled section; every mechanism row is a labelled group whose
 *   state appears IN TEXT (granted / not allowed / unavailable) â€” never
 *   color alone;
 * - degradation tokens render as full localized sentences next to the
 *   mechanism they belong to;
 * - the consent buttons carry the mechanism name in their accessible name,
 *   and revocation stays available while a grant exists (taxonomy Â§3);
 * - the board-conflict warning uses role="alert" because automatic
 *   switching being paused is something the user must know immediately.
 */

import { createElement } from 'react';
import type { ReactElement } from 'react';
import { formatMessage, type MessageCatalog } from '../i18n/catalog.ts';
import type { WorkspaceSnapshot } from '../studio/types.ts';
import type { ConsentAction, MechanismState, SwitchSurfaceState } from './switching-types.ts';

export interface SwitchingProps {
  readonly messages: MessageCatalog;
  /** Latest typed engine state; `null` until the first load resolves. */
  readonly switching: SwitchSurfaceState | null;
  /** Authored documents, used to resolve the active profile's name. */
  readonly snapshot: WorkspaceSnapshot;
}

export interface SwitchingCallbacks {
  onConsent(action: ConsentAction): void;
}

/** Resolves one issue token into a localized sentence. */
export function describeIssue(messages: MessageCatalog, issue: string): string {
  const separator = issue.indexOf(':');
  const kind = separator < 0 ? issue : issue.slice(0, separator);
  const detail = separator < 0 ? '' : issue.slice(separator + 1);
  switch (kind) {
    case 'register-conflict':
      return formatMessage(messages['surface.switching.issue.registerConflict'], {
        combo: detail,
      });
    case 'register-refused':
      return formatMessage(messages['surface.switching.issue.registerRefused'], {
        combo: detail,
      });
    case 'unregister-refused':
      return formatMessage(messages['surface.switching.issue.unregisterRefused'], {
        combo: detail,
      });
    case 'focus-unreadable':
      return messages['surface.switching.issue.focusUnreadable'];
    case 'unsupported':
      return formatMessage(messages['surface.switching.issue.unsupported'], { os: detail });
    default:
      // Unknown token classes still render verbatim: visible, never silent.
      return issue;
  }
}

function mechanismRow(
  key: string,
  messages: MessageCatalog,
  nameKey: keyof MessageCatalog,
  state: MechanismState,
  grantAction: ConsentAction,
  revokeAction: ConsentAction,
  callbacks: SwitchingCallbacks,
): ReactElement {
  const name = messages[nameKey];
  const stateText = !state.supported
    ? messages['surface.switching.unsupported']
    : state.granted
      ? messages['surface.switching.granted']
      : messages['surface.switching.notGranted'];

  const issues =
    state.issues.length === 0
      ? null
      : createElement(
          'ul',
          { className: 'switch-issues' },
          ...state.issues.map((issue, index) =>
            createElement(
              'li',
              { key: `${key}-issue-${index}`, className: 'field-error switch-issue' },
              describeIssue(messages, issue),
            ),
          ),
        );

  return createElement(
    'div',
    { key, className: 'switch-mechanism' },
    createElement('h3', { className: 'panel-subtitle switch-mechanism-name' }, name),
    createElement('p', { className: 'status-line', role: 'status' }, stateText),
    issues,
    state.supported
      ? createElement(
          'button',
          {
            type: 'button',
            className: state.granted ? 'control-button control-button-danger' : 'control-button',
            onClick: () => callbacks.onConsent(state.granted ? revokeAction : grantAction),
          },
          state.granted
            ? formatMessage(messages['surface.switching.revoke'], { mechanism: name })
            : formatMessage(messages['surface.switching.grant'], { mechanism: name }),
        )
      : null,
  );
}

/**
 * Renders the complete profile-switching panel section.
 */
export function renderSwitching(
  props: SwitchingProps,
  callbacks: SwitchingCallbacks,
): ReactElement {
  const { messages, switching, snapshot } = props;
  const headingId = 'switching-heading';

  let body: ReactElement[];
  if (switching === null) {
    body = [
      createElement(
        'p',
        { key: 'loading', className: 'muted', role: 'status' },
        messages['surface.engine.unavailable'],
      ),
    ];
  } else {
    const activeProfile =
      switching.active_profile === null
        ? null
        : (snapshot.profiles.find(
            (document) => document.profile.id === switching.active_profile,
          )?.profile.name ?? switching.active_profile);

    const activeLine = createElement(
      'p',
      {
        key: 'active',
        className: 'status-line switch-active-line',
        role: 'status',
      },
      activeProfile === null
        ? messages['surface.switching.inactive']
        : formatMessage(messages['surface.switching.active'], { name: activeProfile }),
    );

    const conflictAlert = switching.board_conflict
      ? [
          createElement(
            'p',
            { key: 'conflict', role: 'alert', className: 'field-error surface-alert' },
            messages['surface.switching.boardConflict'],
          ),
        ]
      : [];

    body = [
      activeLine,
      ...conflictAlert,
      createElement(
        'div',
        {
          key: 'mechanisms',
          role: 'group',
          className: 'switch-mechanisms',
          'aria-label': messages['surface.switching.mechanisms.label'],
        },
        mechanismRow(
          'hotkey-row',
          messages,
          'surface.switching.hotkey.name',
          switching.hotkeys,
          'grant_hotkey',
          'revoke_hotkey',
          callbacks,
        ),
        mechanismRow(
          'focus-row',
          messages,
          'surface.switching.appFocus.name',
          switching.app_focus,
          'grant_app_focus',
          'revoke_app_focus',
          callbacks,
        ),
      ),
    ];
  }

  return createElement(
    'section',
    {
      className: 'panel surface-panel switching-panel',
      'aria-labelledby': headingId,
    },
    createElement(
      'h2',
      { id: headingId, className: 'panel-title' },
      messages['surface.switching.heading'],
    ),
    ...body,
  );
}
