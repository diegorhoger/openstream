import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  BODY_FONT_SIZE_PX,
  COLOR_TOKEN_NAMES,
  CONTROL_STATES,
  DARK_COLORS,
  DELIVERY_STATES,
  FOCUS_RING,
  LIGHT_COLORS,
  MIN_TEXT_SIZE_PX,
  MOTION,
  NON_COLOR_INDICATOR_RULE,
  RADII_PX,
  SPACING_BASE_PX,
  SPACING_RHYTHM_PX,
  SPACING_STEPS_PX,
  TARGETS,
  THEME_IDS,
  THEMES,
  TOKENS_VERSION,
  TYPOGRAPHY_SCALE_PX,
} from '../design/tokens.ts';
import { CATALOG, LOCALES } from '../i18n/catalog.ts';
import { isHexColor } from '../design/color.ts';

/**
 * Token completeness contract: every vocabulary DESIGN_SYSTEM.md defines
 * must exist as a typed token with exactly the documented value.
 */
describe('design tokens', () => {
  it('expose a semver version', () => {
    assert.match(TOKENS_VERSION, /^\d+\.\d+\.\d+$/);
  });

  it('define the full documented color vocabulary in both themes', () => {
    for (const theme of THEME_IDS) {
      const colors = THEMES[theme];
      assert.deepEqual(
        Object.keys(colors).sort(),
        [...COLOR_TOKEN_NAMES].sort(),
        `${theme} theme color keys`,
      );
    }
  });

  it('use valid hex values verbatim from DESIGN_SYSTEM.md (dark)', () => {
    assert.deepEqual(DARK_COLORS, {
      canvas: '#0B0D0E',
      'surface-1': '#14181A',
      'surface-2': '#1D2326',
      border: '#343C40',
      text: '#F5F7F8',
      'text-muted': '#AAB3B8',
      signal: '#2DE2B4',
      live: '#FF5D5D',
      warning: '#FFB020',
      info: '#58A6FF',
    });
  });

  it('use valid hex values verbatim from DESIGN_SYSTEM.md (light)', () => {
    assert.deepEqual(LIGHT_COLORS, {
      canvas: '#F4F6F7',
      'surface-1': '#FFFFFF',
      'surface-2': '#E9EDEF',
      border: '#C7CED2',
      text: '#111416',
      'text-muted': '#4D585E',
      signal: '#087D65',
      live: '#C82632',
      warning: '#8A5700',
      info: '#0969DA',
    });
  });

  it('keep every color value a well-formed #RRGGBB literal', () => {
    for (const theme of THEME_IDS) {
      for (const [name, value] of Object.entries(THEMES[theme])) {
        assert.ok(isHexColor(value), `${theme}.${name} = ${value}`);
      }
    }
  });

  it('define the exact typography scale with body default and compact floor', () => {
    assert.deepEqual([...TYPOGRAPHY_SCALE_PX], [12, 14, 16, 20, 24, 32, 40]);
    assert.equal(BODY_FONT_SIZE_PX, 16);
    assert.equal(MIN_TEXT_SIZE_PX, 12);
    assert.ok(TYPOGRAPHY_SCALE_PX.includes(MIN_TEXT_SIZE_PX));
    assert.ok(TYPOGRAPHY_SCALE_PX.includes(BODY_FONT_SIZE_PX));
  });

  it('define spacing on the 4 px base with an 8 px primary rhythm', () => {
    assert.equal(SPACING_BASE_PX, 4);
    assert.equal(SPACING_RHYTHM_PX, 8);
    assert.ok(SPACING_STEPS_PX.length >= 1);
    for (const step of SPACING_STEPS_PX) {
      assert.equal(step % SPACING_BASE_PX, 0, `step ${step}px on base`);
    }
  });

  it('define control/panel/deck-key radii', () => {
    assert.deepEqual(RADII_PX, { control: 6, panel: 10, deckKey: 14 });
  });

  it('define the focus ring as 3 px signal with 2 px separation', () => {
    assert.equal(FOCUS_RING.widthPx, 3);
    assert.equal(FOCUS_RING.separationPx, 2);
  });

  it('bound motion to documented budgets', () => {
    assert.ok(MOTION.directManipulationMsMin >= 80);
    assert.ok(MOTION.directManipulationMsMax <= 160);
    assert.ok(MOTION.directManipulationMsMin <= MOTION.directManipulationMsMax);
    assert.equal(MOTION.panelTransitionMs, 200);
  });

  it('define minimum touch targets and desktop deck-key size', () => {
    assert.equal(TARGETS.pointerTargetMinPx, 44);
    assert.deepEqual(
      { w: TARGETS.deckKeyWidthPx, h: TARGETS.deckKeyHeightPx },
      { w: 88, h: 88 },
    );
  });

  it('carry the complete control-state vocabulary', () => {
    assert.deepEqual([...CONTROL_STATES], [
      'idle',
      'hover',
      'focused',
      'pressed',
      'armed',
      'running',
      'succeeded',
      'failed',
      'disabled',
      'unavailable',
      'disconnected',
    ]);
  });

  it('keep relayed/accepted/executed separate delivery states', () => {
    assert.deepEqual([...DELIVERY_STATES], ['relayed', 'accepted', 'executed']);
    const controlIds = new Set<string>(CONTROL_STATES);
    for (const state of DELIVERY_STATES) {
      assert.ok(
        !controlIds.has(state),
        `${state} must not be conflated with a visual control state`,
      );
    }
  });

  it('require a non-color indicator for every state', () => {
    assert.match(NON_COLOR_INDICATOR_RULE, /never|color alone|without color/i);
    assert.ok(NON_COLOR_INDICATOR_RULE.length > 0);
  });
});

/** Localization resource skeleton contract. */
describe('localization catalog', () => {
  it('ships English and Brazilian Portuguese', () => {
    assert.ok(LOCALES.includes('en-US'));
    assert.ok(LOCALES.includes('pt-BR'));
  });

  it('has identical key coverage in every locale', () => {
    const enKeys = Object.keys(CATALOG['en-US'] ?? {}).sort();
    for (const locale of LOCALES) {
      assert.deepEqual(Object.keys(CATALOG[locale] ?? {}).sort(), enKeys);
    }
  });

  it('keeps every message a non-empty trimmed string', () => {
    for (const locale of LOCALES) {
      const catalog = CATALOG[locale] ?? {};
      for (const [key, value] of Object.entries(catalog)) {
        assert.equal(value.trim(), value, `${locale}:${key} trimmed`);
        assert.ok(value.length > 0, `${locale}:${key} non-empty`);
      }
    }
  });
});
