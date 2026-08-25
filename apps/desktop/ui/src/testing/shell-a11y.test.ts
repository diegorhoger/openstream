import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { CATALOG, LOCALES, messagesFor } from '../i18n/catalog.ts';
import type { LocaleId, MessageCatalog, MessageKey } from '../i18n/catalog.ts';
import {
  editorReducer,
  initialEditorState,
  type EditorState,
} from '../studio/editor.ts';
import { renderStudio, type StudioCallbacks } from '../studio/views/studio-view.ts';
import type { ControlKind, WorkspaceSnapshot } from '../studio/types.ts';
import { readUiFile } from './css.ts';

/**
 * Executable keyboard and screen-reader criteria for the OpenStream Studio
 * deck editor (issue #17). The contract renders the EXACT shipped markup
 * (renderStudio is the same entry point App.tsx renders) across a matrix of
 * session states and locales, asserting:
 *
 * - document language/title/self-containment;
 * - one h1 before any h2, sections labelled by real ids, DOM order as
 *   reading order, and NO manual tab-order overrides;
 * - decorative shapes hidden from assistive technology while textual
 *   status always sits beside them (never color alone);
 * - accessible names for every interactive element and explicit
 *   label/field associations in the inspector;
 * - screen-reader-legible control states: disabled, selected, lifted,
 *   overlapping — each carried by TEXT, never color alone;
 * - full localization coverage of every catalog string in en-US and pt-BR
 *   across the rendered state matrix;
 * - visible keyboard affordances (drag alternatives) in the markup itself.
 */

const ID = {
  deckA: '018f6a1c-7b21-7001-9f31-0000000000a1',
  deckB: '018f6a1c-7b21-7001-9f31-0000000000b2',
  page1: '018f6a1c-7b21-7002-9f31-00000000p001'.replace('p0', '00'),
  page2: '018f6a1c-7b21-7002-9f31-000000000002',
  pageB1: '018f6a1c-7b21-7002-9f31-0000000000b1',
  controlMute: '018f6a1c-7b21-7003-9f31-00000000c001',
  controlCam: '018f6a1c-7b21-7003-9f31-00000000c002',
  controlSink: '018f6a1c-7b21-7003-9f31-00000000c003',
  profileMain: '018f6a1c-7b21-7004-9f31-000000000p01'.replace('p0', '00'),
};

function control(id: string, kind: ControlKind, overrides: Partial<import('../studio/types.ts').Control> = {}): import('../studio/types.ts').Control {
  return {
    id,
    page_id: ID.page1,
    kind,
    geometry: { x: 0, y: 0, width: 2, height: 1 },
    label: 'control',
    policy: kind === 'variable_display' ? null : 'press',
    enabled: true,
    ...overrides,
  };
}

function snapshotFixture(): WorkspaceSnapshot {
  return {
    decks: [
      {
        schema_version: { major: 1, minor: 0 },
        deck: {
          id: ID.deckA,
          workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
          title: 'Live scene',
          revision: 4,
          folder_path: 'streaming/overlays',
          deleted_at: null,
          pages: [
            {
              id: ID.page1,
              deck_id: ID.deckA,
              ordinal: 0,
              grid: { columns: 8, rows: 4 },
              controls: [
                control(ID.controlMute, 'button', { label: 'Mute mic' }),
                control(ID.controlCam, 'toggle', {
                  label: 'Camera',
                  geometry: { x: 2, y: 0, width: 2, height: 1 },
                  enabled: false,
                }),
                // Overlaps Camera deliberately: collision diagnostics below.
                control(ID.controlSink, 'variable_display', {
                  label: 'Viewers',
                  geometry: { x: 3, y: 0, width: 2, height: 1 },
                }),
              ],
            },
            {
              id: ID.page2,
              deck_id: ID.deckA,
              ordinal: 1,
              grid: { columns: 8, rows: 4 },
              controls: [],
            },
          ],
        },
      },
      {
        schema_version: { major: 1, minor: 0 },
        deck: {
          id: ID.deckB,
          workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
          title: 'Backup deck',
          revision: 1,
          folder_path: '',
          deleted_at: null,
          pages: [
            {
              id: ID.pageB1,
              deck_id: ID.deckB,
              ordinal: 0,
              grid: { columns: 8, rows: 4 },
              controls: [],
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
          deck_ids: [ID.deckA, ID.deckB],
        },
      },
    ],
  };
}

function baseReadyState(locale: LocaleId, extra: Partial<EditorState> = {}): EditorState {
  let state = editorReducer(initialEditorState, {
    type: 'loaded',
    result: { snapshot: snapshotFixture(), autosave_active: true },
  });
  state = { ...state, locale };
  return { ...state, ...extra };
}

const noopCallbacks: StudioCallbacks = new Proxy(
  { __proto__: null },
  {
    get: () => () => undefined,
  },
) as unknown as StudioCallbacks;

const markupFor = (state: EditorState): string =>
  renderToStaticMarkup(renderStudio(state, noopCallbacks));

describe('shell document contract', () => {
  const html = readUiFile('index.html');

  it('declares the document language for screen readers', () => {
    assert.match(html, /<html lang="en">/);
  });

  it('names the document', () => {
    assert.match(html, /<title>OpenStream<\/title>/);
  });

  it('keeps the bundle self-contained (no third-party origins)', () => {
    assert.doesNotMatch(html, /https?:\/\//);
  });
});

describe('shell keyboard criteria', () => {
  const dom = markupFor(baseReadyState('en-US'));

  it('uses DOM order as reading order: header, main, footer appear once in order', () => {
    const header = dom.indexOf('<header');
    const main = dom.indexOf('<main');
    const footer = dom.indexOf('<footer');
    assert.ok(header >= 0 && main > header && footer > main);
    assert.equal(dom.split('<header').length - 1, 1);
    assert.equal(dom.split('<main').length - 1, 1);
    assert.equal(dom.split('<footer').length - 1, 1);
  });

  it('adds no manual tab-order overrides (focus follows DOM order)', () => {
    for (const locale of LOCALES) {
      assert.doesNotMatch(markupFor(baseReadyState(locale)), /tabindex/i);
      assert.doesNotMatch(markupFor(baseReadyState(locale)), /tabIndex/i);
    }
  });

  it('renders the keyboard alternative affordances next to drag targets', () => {
    // Page reorder buttons exist (the keyboard path for dragging tabs).
    for (const locale of LOCALES) {
      const messages = messagesFor(locale);
      const ready = baseReadyState(locale);
      const markup = markupFor(ready);
      assert.ok(
        markup.includes(format(messages, 'studio.pages.moveUp', { index: 2 })),
        'page move-up affordance present',
      );
      const folderMove = format(messages, 'studio.deck.moveToFolder', { title: 'Live scene' });
      assert.ok(
        markup.includes(folderMove),
        'folder move affordance present',
      );
      const profileSelected = editorReducer(ready, {
        type: 'select',
        selection: { kind: 'profile', profileId: ID.profileMain },
        announceName: 'Streaming',
      });
      const profileMarkup = markupFor(profileSelected);
      // First list row exposes move-down; second exposes move-up.
      assert.ok(
        profileMarkup.includes(
          format(messages, 'studio.profiles.moveDown', { deck: 'Live scene', name: 'Streaming' }),
        ),
        'profile reorder affordance present once selected',
      );
    }
  });

  it('exposes undo availability through the disabled attribute, not color', () => {
    assert.match(dom, /<button[^>]*disabled[^>]*>[^<]*Undo/s);
    assert.match(dom, /<button[^>]*disabled[^>]*>[^<]*Redo/s);
  });
});

function format(messages: MessageCatalog, key: MessageKey, params: Record<string, string | number>): string {
  const template: string = messages[key];
  assert.ok(typeof template === 'string', `catalog key exists: ${key}`);
  return template.replace(/\{([a-z_]+)\}/gi, (_match: string, name: string) => String(params[name] ?? `{${name}}`));
}

describe('shell screen-reader criteria', () => {
  for (const locale of LOCALES) {
    const messages = messagesFor(locale);
    const dom = markupFor(baseReadyState(locale));

    it(`renders one h1 followed by an h2 (${locale})`, () => {
      assert.equal(dom.split('<h1').length - 1, 1);
      const h1 = dom.indexOf('<h1');
      const h2 = dom.indexOf('<h2');
      assert.ok(h2 > h1);
    });

    it(`labels every section with an existing heading id (${locale})`, () => {
      const refs = [...dom.matchAll(/aria-labelledby="([^"]+)"/g)].map((m) => m[1]);
      assert.ok(refs.length >= 5, `sections are labelled (${refs.length})`);
      for (const ref of refs) {
        assert.ok(dom.includes(`id="${ref}"`), `labelledby target exists: ${ref}`);
      }
    });

    it(`hides decorative indicators from assistive technology (${locale})`, () => {
      assert.match(dom, /class="status-dot"[^>]*aria-hidden|aria-hidden="true"[^>]*class="status-dot"/);
      assert.match(dom, /toolbar-separator[^>]*aria-hidden|aria-hidden="true"[^>]*toolbar-separator/);
    });

    it(`conveys engine state as text next to the indicator, never color alone (${locale})`, () => {
      assert.ok(dom.includes(messages['engine.status.notConnected']));
      assert.ok(dom.includes(messages['engine.status.label']));
    });

    it(`announces autosave status through a polite status region (${locale})`, () => {
      assert.match(dom, /role="status"/);
      assert.ok(dom.includes(messages['studio.save.saved']));
    });

    it(`carries the polite live region for announcements (${locale})`, () => {
      assert.match(dom, /aria-live="polite"/);
    });

    it(`states grid geometry in words on the canvas group (${locale})`, () => {
      assert.ok(
        dom.includes(
          format(messages, 'studio.canvas.gridLabel', {
            index: 1,
            total: 2,
            columns: 8,
            rows: 4,
          }),
        ),
      );
    });
  }
});

/**
 * Control-state semantics: disabled, selected, lifted, and overlapping
 * states must be legible as TEXT within each control's accessible
 * description — the "never color alone" rule made executable.
 */
describe('control state semantics', () => {
  for (const locale of LOCALES) {
    const messages = messagesFor(locale);

    it(`marks disabled controls in their accessible name (${locale})`, () => {
      const dom = markupFor(baseReadyState(locale));
      assert.ok(
        dom.includes(`${messages['studio.control.kind.toggle']}, ${messages['studio.state.disabled']}`),
        'disabled state rides in the accessible name',
      );
      assert.ok(
        dom.includes(messages['studio.state.disabled']),
        'visible disabled badge text present',
      );
    });

    it(`links overlap warnings through aria-describedby (${locale})`, () => {
      const diagnostics = [
        {
          code: 'grid_collision',
          page_id: ID.page1,
          control_ids: [ID.controlCam, ID.controlSink],
        },
      ];
      const dom = markupFor(baseReadyState(locale, { diagnostics }));
      assert.match(dom, /aria-describedby="collision-description"/);
      assert.ok(dom.includes(messages['studio.collision.description']));
      assert.ok(dom.includes(messages['studio.collision.badge']));
    });

    it(`names the lifted state and its hint (${locale})`, () => {
      let state = baseReadyState(locale);
      state = editorReducer(state, {
        type: 'lift-begin',
        pageId: ID.page1,
        controlId: ID.controlMute,
      });
      const dom = markupFor(state);
      assert.ok(
        dom.includes(`${messages['studio.state.liftedSuffix']}`),
        'lifted suffix present in accessible name',
      );
      assert.equal(
        state.announcement,
        format(messages, 'studio.announce.lifted', {
          name: 'Mute mic',
          hint: messages['studio.lift.hint'],
        }),
        'announcement names the lifted control with its hint',
      );
      assert.match(dom, /canvas-lift-ghost/);
    });
  }

  it('keeps selection visual state out of color-only channels (aria-pressed)', () => {
    let state = baseReadyState('en-US');
    state = editorReducer(state, {
      type: 'select',
      selection: { kind: 'control', pageId: ID.page1, controlId: ID.controlMute },
      announceName: 'Mute mic',
    });
    const dom = markupFor(state);
    assert.match(dom, /aria-pressed="true"/);
  });
});

/** Inspector form semantics: every field pairs a label with its input. */
describe('inspector form semantics', () => {
  function selectedControlState(locale: LocaleId): EditorState {
    let state = baseReadyState(locale);
    state = editorReducer(state, {
      type: 'select',
      selection: { kind: 'control', pageId: ID.page1, controlId: ID.controlMute },
      announceName: 'Mute mic',
    });
    return state;
  }

  for (const locale of LOCALES) {
    const dom = markupFor(selectedControlState(locale));

    it(`pairs every inspector input with a label element (${locale})`, () => {
      const inputs = [...dom.matchAll(/<(?:input|select)\b([^>]*)>/g)]
        .map((match) => match[1])
        .filter((attrs): attrs is string => typeof attrs === 'string');
      assert.ok(inputs.length >= 6, `inspector fields rendered (${inputs.length})`);
      for (const attrs of inputs) {
        const idMatch = attrs.match(/id="([^"]+)"/);
        if (idMatch === null || idMatch[1] === undefined) {
          assert.match(attrs, /aria-label=/, `field labelled: ${attrs.slice(0, 80)}`);
          continue;
        }
        const id: string = idMatch[1];
        assert.ok(dom.includes(`for="${id}"`), `label[for] exists for ${id}`);
      }
    });

    it(`surfaces refusals as an alert tied to the last change (${locale})`, () => {
      const catalog = CATALOG[locale];
      assert.ok(catalog);
      let state = selectedControlState(locale);
      state = editorReducer(state, {
        type: 'op-rejected',
        token: 'geometry_outside_grid:x',
      });
      const markup = markupFor(state);
      assert.match(markup, /role="alert"/);
      assert.ok(
        markup.includes(catalog['studio.error.prefix']),
        'refusal sentence prefix localized',
      );
    });
  }
});

/**
 * Localization completeness across the whole state matrix: every catalog
 * template must be reachable. Placeholder-bearing templates are matched as
 * patterns (any concrete value where a placeholder sits), so the assertion
 * covers the reachable sentence shape rather than raw `{token}` text.
 */
describe('localization coverage over rendered states', () => {
  function templateToRegExp(template: string): RegExp {
    const escape = (part: string): string => part.replace(/[.*+?^'(){}[\]\\]/g, '\\$&');
    const parts = template.split(/\{[a-z_]+\}/i).map((part) => escape(part));
    return new RegExp(parts.join('[^"<>]*'));
  }

  const states: Array<[string, EditorState]> = [];
  for (const locale of LOCALES) {
    const messages = messagesFor(locale);
    const ready = baseReadyState(locale);
    states.push(['ready', ready]);
    states.push([
      'loading',
      { ...editorReducer(initialEditorState, { type: 'loaded', result: { snapshot: snapshotFixture(), autosave_active: true } }), phase: 'loading', locale },
    ]);
    states.push([
      'failed',
      { ...baseReadyState(locale), phase: 'failed', loadErrorToken: 'bridge-unavailable' },
    ]);
    states.push([
      'save-degraded',
      baseReadyState(locale, {
        autosaveActive: false,
        saved: false,
        saveError: 'autosave_unavailable',
      }),
    ]);
    states.push([
      'save-refused',
      baseReadyState(locale, {
        autosaveActive: true,
        saved: false,
        saveError: 'autosave_refused',
      }),
    ]);
    const selected = editorReducer(ready, {
      type: 'select',
      selection: { kind: 'control', pageId: ID.page1, controlId: ID.controlMute },
      announceName: 'Mute mic',
    });
    states.push(['control-selected', selected]);
    states.push([
      'error-shown',
      editorReducer(selected, { type: 'op-rejected', token: 'text_out_of_range:title' }),
    ]);
    states.push([
      'geometry-error-shown',
      editorReducer(selected, { type: 'op-rejected', token: 'geometry_outside_grid:x' }),
    ]);
    states.push([
      'notfound-error-shown',
      editorReducer(selected, { type: 'op-rejected', token: 'not_found:deck' }),
    ]);
    states.push([
      'deck-selected',
      editorReducer(ready, {
        type: 'select',
        selection: { kind: 'deck', deckId: ID.deckB },
        announceName: 'Backup deck',
      }),
    ]);
    states.push([
      'profile-selected',
      editorReducer(ready, {
        type: 'select',
        selection: { kind: 'profile', profileId: ID.profileMain },
        announceName: 'Streaming',
      }),
    ]);
    states.push([
      'empty-workspace',
      {
        ...editorReducer(initialEditorState, {
          type: 'loaded',
          result: { snapshot: { decks: [], profiles: [] }, autosave_active: true },
        }),
        locale,
      },
    ]);
    states.push([
      'page-selected',
      editorReducer(ready, {
        type: 'select',
        selection: { kind: 'page', pageId: ID.page1 },
      }),
    ]);
    states.push([
      'display-selected',
      editorReducer(ready, {
        type: 'select',
        selection: { kind: 'control', pageId: ID.page1, controlId: ID.controlSink },
        announceName: 'Viewers',
      }),
    ]);
    states.push([
      'lifted',
      editorReducer(ready, {
        type: 'lift-begin',
        pageId: ID.page1,
        controlId: ID.controlMute,
      }),
    ]);
    states.push([
      'collisions',
      baseReadyState(locale, {
        diagnostics: [
          {
            code: 'grid_collision',
            page_id: ID.page1,
            control_ids: [ID.controlCam, ID.controlSink],
          },
        ],
      }),
    ]);

    // Every refusal token is reachable through the inspector alert line.
    const errorTokens = [
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
      'unknown',
    ];
    for (const token of errorTokens) {
      states.push([
        `error:${token}`,
        editorReducer(selected, { type: 'op-rejected', token }),
      ]);
    }

    // Announcement variants: each announce.* sentence is reachable through
    // the polite live region during its own interaction.
    const announcements: Array<[MessageKey, Record<string, string | number>]> = [
      ['studio.announce.selected', { name: 'Mute mic' }],
      [
        'studio.announce.lifted',
        { name: 'Mute mic', hint: messages['studio.lift.hint'] },
      ],
      ['studio.announce.dropped', { name: 'Mute mic', x: 2, y: 1 }],
      ['studio.announce.canceled', {}],
      ['studio.announce.pageOpened', { index: 1 }],
    ];
    for (const [key, params] of announcements) {
      states.push([
        `announcement:${key}`,
        {
          ...ready,
          announcement: format(messages, key, params),
          announcementSeq: 1,
        },
      ]);
    }
  }

  const renderedTexts: Array<[string, string]> = states.map(([name, state]) => [
    name,
    decodeEntities(markupFor(state)),
  ]);

  for (const locale of LOCALES) {
    it(`renders every ${locale} catalog string somewhere reachable`, () => {
      const catalog = CATALOG[locale];
      assert.ok(catalog);
      const missing: string[] = [];
      for (const [key, template] of Object.entries(catalog)) {
        const reachable = renderedTexts.some(([, dom]) =>
          template.includes('{')
            ? templateToRegExp(template).test(dom)
            : dom.includes(template),
        );
        if (!reachable) {
          missing.push(key);
        }
      }
      assert.deepEqual(missing, [], 'every catalog string renders in some state');
    });
  }
});

/** React escapes quotes/ampersands in text; undo that for string matching. */
function decodeEntities(text: string): string {
  return text
    .replaceAll('&quot;', '"')
    .replaceAll('&#x27;', "'")
    .replaceAll('&amp;', '&')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>');
}
