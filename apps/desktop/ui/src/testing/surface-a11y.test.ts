import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import {
  editorReducer,
  initialEditorState,
  type EditorState,
} from '../studio/editor.ts';
import { CATALOG, LOCALES, formatMessage, messagesFor, type LocaleId } from '../i18n/catalog.ts';
import { renderStudio, type StudioCallbacks } from '../studio/views/studio-view.ts';
import { INITIAL_KEY_RUNTIME, type ExecutionPhase } from '../surface/machine.ts';
import {
  PHASE_GLYPHS,
  describeRefusal,
  phaseAnnouncement,
  renderSurface,
} from '../surface/views/surface-view.ts';
import { renderSwitching } from '../surface/switching-view.ts';
import type {
  ConsentAction,
  SwitchSurfaceState,
} from '../surface/switching-types.ts';

/**
 * Executable accessibility contract for the live deck surface (issue #18).
 *
 * Renders the EXACT shipped composition (renderStudio in live mode with
 * renderSurface as its main content) across every execution phase and both
 * locales, asserting:
 *
 * - document heading order and labelled sections;
 * - every deck key carries label + kind + phase IN TEXT with an
 *   aria-hidden glyph — status never rides on color alone;
 * - relayed / accepted / running / terminals each render DISTINCT badge
 *   words, and nothing but an authoritative phase ever shows success;
 * - armed keys resolve only through a named confirm/cancel group announced
 *   via role="alert" (no timeout);
 * - failures announce through role="alert", transitions through one polite
 *   live region, engine availability through role="status";
 * - no manual tab-order overrides anywhere;
 * - complete localization coverage of every new catalog string per locale.
 */

const ID = {
  deckA: '018f6a1c-7b21-7001-9f31-0000000000a1',
  page1: '018f6a1c-7b21-7002-9f31-000000000001',
  mute: '018f6a1c-7b21-7003-9f31-00000000c001',
  camera: '018f6a1c-7b21-7003-9f31-00000000c002',
  viewers: '018f6a1c-7b21-7003-9f31-00000000c003',
  profileMain: '018f6a1c-7b21-7004-9f31-000000000001',
};

const ALL_PHASES: readonly ExecutionPhase[] = [
  'idle',
  'pressed',
  'armed',
  'relayed',
  'accepted',
  'running',
  'succeeded',
  'failed',
  'cancelled',
  'expired',
  'outcome_unknown',
];

function snapshotFixture(): Parameters<typeof editorReducer>[0]['snapshot'] {
  void snapshotFixtureName;
  return {
    decks: [
      {
        schema_version: { major: 1, minor: 0 },
        deck: {
          id: ID.deckA,
          workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
          title: 'Live scene',
          revision: 1,
          folder_path: '',
          deleted_at: null,
          pages: [
            {
              id: ID.page1,
              deck_id: ID.deckA,
              ordinal: 0,
              grid: { columns: 8, rows: 4 },
              controls: [
                {
                  id: ID.mute,
                  page_id: ID.page1,
                  kind: 'button',
                  geometry: { x: 0, y: 0, width: 2, height: 1 },
                  label: 'Mute mic',
                  policy: 'press',
                  enabled: true,
                },
                {
                  id: ID.camera,
                  page_id: ID.page1,
                  kind: 'toggle',
                  geometry: { x: 2, y: 0, width: 2, height: 1 },
                  label: 'Camera',
                  policy: 'toggle',
                  enabled: true,
                },
                {
                  id: ID.viewers,
                  page_id: ID.page1,
                  kind: 'variable_display',
                  geometry: { x: 4, y: 0, width: 2, height: 1 },
                  label: 'Viewers',
                  policy: null,
                  enabled: true,
                },
              ],
            },
          ],
        },
      },
    ],
    profiles: [
      {
        schema_version: { major: 1, minor: 0 },
        profile: {
          id: ID.profileMain,
          workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
          name: 'Streaming',
          deck_ids: [ID.deckA],
          switch_rules: [],
        },
      },
    ],
  };
}

function snapshotFixtureName(): void {}

function baseReadyState(locale: LocaleId): EditorState {
  let state = editorReducer(initialEditorState, {
    type: 'loaded',
    result: { snapshot: snapshotFixture(), autosave_active: true },
  });
  state = { ...state, locale };
  return editorReducer(state, { type: 'mode-changed', mode: 'live' });
}

const noopCallbacks: StudioCallbacks = new Proxy(
  { __proto__: null },
  { get: () => () => undefined },
) as unknown as StudioCallbacks;

function surfaceMarkup(
  locale: LocaleId,
  options: {
    mutePhase?: ExecutionPhase;
    alert?: string;
    announcement?: string;
    announcementSeq?: number;
    engineAvailable?: boolean;
    empty?: boolean;
    latched?: boolean;
  } = {},
): string {
  const messages = messagesFor(locale);
  const runtimes = {
    [ID.mute]: {
      ...INITIAL_KEY_RUNTIME,
      phase: options.mutePhase ?? 'idle',
      failureToken:
        options.mutePhase === 'failed' || options.mutePhase === undefined
          ? (options.mutePhase === 'failed' ? 'binding_absent' : null)
          : null,
    },
    [ID.camera]: { ...INITIAL_KEY_RUNTIME, latched: options.latched === true },
  };
  const liveContent = renderSurface(
    {
      messages,
      snapshot: options.empty === true
        ? { decks: [], profiles: [] }
        : snapshotFixture(),
      currentPageId: ID.page1,
      engineAvailable: options.engineAvailable ?? true,
      runtimes,
      armedControlIds: options.mutePhase === 'armed' ? [ID.mute] : [],
      announcement: options.announcement ?? '',
      alert: options.alert ?? '',
      announcementSeq: options.announcementSeq ?? 0,
    },
    {
      onPageSelect: () => undefined,
      onPressBegin: () => undefined,
      onPressEnd: () => undefined,
      onArmConfirm: () => undefined,
      onArmCancel: () => undefined,
      onSurfaceKeyDown: () => undefined,
    },
  );
  return renderToStaticMarkup(renderStudio(baseReadyState(locale), noopCallbacks, liveContent));
}

/** Scripted switching engine state for the switching-panel contract. */
function switchingFixture(options: {
  granted?: boolean;
  supported?: boolean;
  issues?: string[];
  boardConflict?: boolean;
  active?: boolean;
}): SwitchSurfaceState {
  const granted = options.granted ?? true;
  const supported = options.supported ?? true;
  return {
    active_profile: options.active === false ? null : ID.profileMain,
    hotkeys: {
      granted,
      supported,
      issues: options.issues ?? [],
    },
    app_focus: { granted, supported, issues: [] },
    rule_count: 1,
    board_conflict: options.boardConflict ?? false,
  };
}

function switchingMarkup(
  locale: LocaleId,
  state: SwitchSurfaceState = switchingFixture({}),
): string {
  const messages = messagesFor(locale);
  const panel = renderSwitching(
    { messages, switching: state, snapshot: snapshotFixture() },
    { onConsent: (_action: ConsentAction) => undefined },
  );
  return renderToStaticMarkup(panel);
}

describe('surface document contract', () => {
  it('keeps one h1 before the labelled surface section', () => {
    for (const locale of LOCALES) {
      const dom = surfaceMarkup(locale);
      assert.ok(dom.indexOf('<h1') < dom.indexOf('id="surface-heading"'));
      assert.ok(dom.includes(`>${messagesFor(locale)['surface.heading']}</h2>`));
    }
  });

  it('adds no manual tab-order overrides in live mode', () => {
    for (const locale of LOCALES) {
      assert.doesNotMatch(surfaceMarkup(locale), /tabindex/i);
    }
  });

  it('states engine availability in text next to the decorative dot', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      assert.ok(surfaceMarkup(locale).includes(messages['surface.engine.ready']));
      const down = surfaceMarkup(locale, { engineAvailable: false });
      assert.ok(down.includes(messages['surface.engine.unavailable']));
    }
  });

  it('labels pages tabs and marks the current one with aria-current', () => {
    const dom = surfaceMarkup('en-US');
    assert.ok(dom.includes('aria-current="true"'));
    assert.ok(dom.includes('Pages of this deck'));
  });

  it('states grid geometry in words', () => {
    const dom = surfaceMarkup('en-US');
    assert.match(dom, /Page 1 of 1: 8 columns by 4 rows/);
  });
});

describe('every execution phase renders icon AND text distinctly', () => {
  for (const phase of ALL_PHASES) {
    it(`renders ${phase}`, () => {
      for (const locale of LOCALES) {
        const messages = messagesFor(locale);
        const dom = surfaceMarkup(locale, { mutePhase: phase });
        if (phase !== 'idle') {
          const badgeText = messages[`surface.phase.${phase}` as keyof typeof messages];
          assert.ok(dom.includes(badgeText), `${phase}: missing badge "${badgeText}"`);
          // Distinctness: each phase's badge word differs from every other's.
          const others = ALL_PHASES.filter((candidate) => candidate !== phase && candidate !== 'idle')
            .map((candidate) => messages[`surface.phase.${candidate}` as keyof typeof messages]);
          assert.ok(!others.includes(badgeText), `${phase} shares a badge word`);
        }
        // The decorative glyph is hidden from assistive technology…
        const glyph = PHASE_GLYPHS[phase];
        if (glyph.length > 0) {
          assert.ok(
            dom.includes(`<span aria-hidden="true" class="key-glyph phase-${phase}">${glyph}</span>`) ||
              dom.includes(`class="key-glyph phase-${phase}"`),
            `${phase}: glyph present`,
          );
        }
        // …while the accessible name carries the state in words.
        const kindLabel = messages['studio.control.kind.button'];
        const phaseWord = messages[`surface.phase.${phase}` as keyof typeof messages];
        const nameSuffix =
          phase === 'idle'
            ? `Mute mic, ${kindLabel}`
            : `Mute mic, ${kindLabel}, ${phaseWord}`;
        assert.ok(
          dom.includes(nameSuffix),
          `${phase}: accessible name must end with the state`,
        );
      }
    });
  }

  it('never presents success from anything but the authoritative phase', () => {
    for (const locale of LOCALES) {
      const succeededWord = messagesFor(locale)['surface.phase.succeeded'];
      for (const phase of ['idle', 'pressed', 'relayed', 'accepted'] as const) {
        assert.ok(
          !surfaceMarkup(locale, { mutePhase: phase }).includes(succeededWord),
          `${phase} must not show success`,
        );
      }
    }
  });
});

describe('destructive arming contract', () => {
  it('announces armed keys and resolves only via confirm/cancel', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const dom = surfaceMarkup(locale, { mutePhase: 'armed' });
      assert.ok(dom.includes(messages['surface.arming.group']));
      assert.ok(dom.includes('role="alert"'), 'arming announces assertively');
      assert.ok(dom.includes(messages['surface.arming.confirm'].replace('{name}', 'Mute mic')));
      assert.ok(dom.includes(messages['surface.arming.cancel'].replace('{name}', 'Mute mic')));
      assert.ok(
        !dom.includes(messages['surface.phase.relayed']),
        'an unarmed-confirmed key cannot be in flight',
      );
    }
  });

  it('shows no arming strip when nothing is armed', () => {
    assert.ok(!surfaceMarkup('en-US').includes('Confirm destructive action'));
  });
});

describe('failure and announcement regions', () => {
  it('renders refusals through role=alert with localized sentences', () => {
    const token = 'binding_absent';
    const sentence = describeRefusal(CATALOG['en-US'], token);
    assert.ok(sentence.length > 10);
    const dom = surfaceMarkup('en-US', {
      mutePhase: 'failed',
      alert: sentence,
      announcementSeq: 2,
    }).replace(/&#x27;/g, "'");
    assert.ok(dom.includes('role="alert"'));
    assert.ok(dom.includes(sentence));
  });

  it('keeps one polite live region for transitions', () => {
    const dom = surfaceMarkup('en-US', {
      announcement: phaseAnnouncement(CATALOG['en-US'], 'Mute mic', 'relayed', null),
      announcementSeq: 2,
    });
    assert.ok(dom.includes('aria-live="polite"'));
    assert.ok(dom.includes('Mute mic sent to the Engine.'));
  });

  it('describes value sinks instead of inviting interaction', () => {
    const dom = surfaceMarkup('en-US');
    assert.ok(dom.includes('Value display: shows a variable value; it takes no input.'));
    assert.ok(/disabled/.test(dom), 'state sink renders inert');
  });
});

describe('switching panel contract (issue #19)', () => {
  it('renders a labelled section with heading order intact', () => {
    for (const locale of LOCALES) {
      const dom = switchingMarkup(locale);
      assert.ok(dom.includes('id="switching-heading"'));
      assert.ok(dom.includes(`>${messagesFor(locale)['surface.switching.heading']}</h2>`));
      assert.doesNotMatch(dom, /tabindex/i);
    }
  });

  it('states mechanism authority in words and names consent buttons', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const hotkeyName = messages['surface.switching.hotkey.name'];
      const grantLabel = formatMessage(messages['surface.switching.grant'], {
        mechanism: hotkeyName,
      });
      const revokeLabel = formatMessage(messages['surface.switching.revoke'], {
        mechanism: hotkeyName,
      });
      const granted = switchingMarkup(locale, switchingFixture({ granted: true }));
      assert.ok(granted.includes(revokeLabel));
      assert.ok(granted.includes(messages['surface.switching.granted']));
      const denied = switchingMarkup(locale, switchingFixture({ granted: false }));
      assert.ok(denied.includes(messages['surface.switching.notGranted']));
      assert.ok(denied.includes(grantLabel));
    }
  });

  it('never renders consent controls on unsupported platforms', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const dom = switchingMarkup(
        locale,
        switchingFixture({ supported: false, issues: [`unsupported:${'windows'}`] }),
      );
      assert.ok(dom.includes(messages['surface.switching.unsupported']));
      const revokeLabel = formatMessage(messages['surface.switching.revoke'], {
        mechanism: messages['surface.switching.hotkey.name'],
      });
      assert.ok(!dom.includes(revokeLabel));
    }
  });

  it('alerts when conflicting rules pause automatic switching', () => {
    for (const locale of LOCALES) {
      const dom = switchingMarkup(locale, switchingFixture({ boardConflict: true }));
      assert.ok(dom.includes('role="alert"'));
      assert.ok(dom.includes(messagesFor(locale)['surface.switching.boardConflict']));
    }
  });

  it('localizes degradation tokens next to their mechanism', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const dom = switchingMarkup(
        locale,
        switchingFixture({
          issues: ['register-conflict:ctrl+shift+f5', 'focus-unreadable'],
        }),
      );
      assert.ok(dom.includes('ctrl+shift+f5'));
      assert.ok(dom.includes(messages['surface.switching.issue.focusUnreadable']));
    }
  });

  it('resolves the active profile name and its absence honestly', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const active = switchingMarkup(locale);
      assert.ok(active.includes('Streaming'));
      const inactive = switchingMarkup(locale, switchingFixture({ active: false }));
      assert.ok(inactive.includes(messages['surface.switching.inactive']));
    }
  });
});

describe('localization coverage', () => {
  it('covers every new surface string somewhere in the phase × locale matrix', () => {
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const perPhase: string[] = [];
      for (const phase of ALL_PHASES) {
        perPhase.push(
          surfaceMarkup(locale, {
            mutePhase: phase,
            alert:
              phase === 'failed'
                ? phaseAnnouncement(messages, 'Mute mic', 'failed', 'binding_absent')
                : '',
            announcement: phaseAnnouncement(messages, 'Mute mic', phase, 'binding_absent'),
            announcementSeq: 2,
          }),
        );
      }
      const corpus = [
        ...perPhase,
        // App-glue composed refusal sentences (App maps tokens through
        // describeRefusal into the same alert line).
        describeRefusal(messages, 'binding_absent'),
        describeRefusal(messages, 'control_disabled'),
        describeRefusal(messages, 'state_sink_no_interaction'),
        describeRefusal(messages, 'policy_mismatch:press'),
        describeRefusal(messages, 'something_new'),
        surfaceMarkup(locale, { engineAvailable: false }),
        surfaceMarkup(locale, { empty: true }),
        surfaceMarkup(locale, { latched: true, announcementSeq: 2 }),
        // Issue #19 switching panel across its visible states.
        switchingMarkup(locale),
        switchingMarkup(locale, switchingFixture({ granted: false })),
        switchingMarkup(locale, switchingFixture({ supported: false, issues: ['unsupported:linux'] })),
        switchingMarkup(locale, switchingFixture({ boardConflict: true, active: false })),
        switchingMarkup(locale, switchingFixture({ issues: [
          'register-conflict:ctrl+shift+f5',
          'register-refused:ctrl+alt+f5',
          'unregister-refused:ctrl+alt+f6',
          'focus-unreadable',
        ] })),
      ]
        .join('\n')
        .replace(/&#x27;/g, "'");
      const rendered = [
        ...Object.keys(CATALOG[locale])
          .filter((key) => key.startsWith('surface.') || key.startsWith('studio.mode') || key.startsWith('studio.toolbar.mode'))
          .map((key) => key as keyof typeof messages),
      ]
        .map((key) => messages[key])
        .filter((text) => text.length >= 8);
      const matches = (template: string): boolean => {
        const escaped = template.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        const pattern = escaped.replace(/\\\{[a-z_]+\\\}/g, '.{1,120}');
        return new RegExp(pattern).test(corpus);
      };
      for (const text of rendered) {
        assert.ok(
          matches(text),
          `missing rendered form for "${text}"`,
        );
      }
    }
  });
});
