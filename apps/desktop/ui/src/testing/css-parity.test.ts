import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  FOCUS_RING,
  MOTION,
  RADII_PX,
  SPACING_BASE_PX,
  SPACING_RHYTHM_PX,
  SPACING_STEPS_PX,
  TARGETS,
  THEMES,
  TYPOGRAPHY_SCALE_PX,
  MONO_FONT_STACK,
  UI_FONT_STACK,
} from '../design/tokens.ts';
import { parseCssRules, readUiFile } from './css.ts';

/**
 * CSS parity contract: styles/tokens.css must mirror the typed token
 * module exactly, and the shell stylesheet must implement the focus
 * visibility and reduced-motion rules.
 */

function declarationsFor(css: string, selector: string): Map<string, string> {
  const rule = parseCssRules(css).find((entry) => entry.selector === selector);
  if (!rule) {
    throw new Error(`selector not found: ${selector}`);
  }
  return rule.declarations;
}

const colorVars = (colors: typeof THEMES.dark): string[] =>
  Object.keys(colors).map((name) => `--${name}`);

describe('tokens.css mirrors the typed tokens', () => {
  const css = readUiFile('src/styles/tokens.css');

  it('defines the dark palette on :root', () => {
    const root = declarationsFor(css, ':root');
    for (const [name, value] of Object.entries(THEMES.dark)) {
      assert.equal(root.get(`--${name}`), value.toLowerCase(), `--${name}`);
    }
  });

  it('defines the light palette on [data-theme="light"]', () => {
    const light = declarationsFor(css, "[data-theme='light']");
    for (const [name, value] of Object.entries(THEMES.light)) {
      assert.equal(light.get(`--${name}`), value.toLowerCase(), `--${name}`);
    }
  });

  it('exposes exactly the documented color vocabulary on :root', () => {
    const root = declarationsFor(css, ':root');
    const colorProps = [...root.keys()].filter((prop) =>
      Object.keys(THEMES.dark).includes(prop.slice(2)),
    );
    assert.deepEqual(
      colorProps.sort(),
      colorVars(THEMES.dark).sort(),
      'color custom properties on :root',
    );
  });

  it('exposes exactly the documented color vocabulary on light override', () => {
    const light = declarationsFor(css, "[data-theme='light']");
    assert.ok(light.size >= 10);
    for (const name of Object.keys(THEMES.light)) {
      assert.ok(light.has(`--${name}`), `--${name}`);
    }
    const allowed = new Set([
      ...colorVars(THEMES.light),
      'color-scheme',
    ]);
    for (const prop of light.keys()) {
      assert.ok(
        allowed.has(prop),
        `light override carries only theme colors plus scheme: unexpected ${prop}`,
      );
    }
  });

  it('mirrors the typography scale', () => {
    const root = declarationsFor(css, ':root');
    for (const size of TYPOGRAPHY_SCALE_PX) {
      assert.equal(root.get(`--font-size-${size}`), `${size}px`);
    }
  });

  it('mirrors font stacks', () => {
    const root = declarationsFor(css, ':root');
    assert.equal(root.get('--font-ui'), UI_FONT_STACK);
    assert.equal(root.get('--font-mono'), MONO_FONT_STACK);
  });

  it('mirrors spacing base, rhythm, and steps', () => {
    const root = declarationsFor(css, ':root');
    assert.equal(root.get('--space-base'), `${SPACING_BASE_PX}px`);
    assert.equal(root.get('--space-rhythm'), `${SPACING_RHYTHM_PX}px`);
    for (const step of SPACING_STEPS_PX) {
      assert.equal(root.get(`--space-${step}`), `${step}px`, `--space-${step}`);
    }
  });

  it('mirrors radii', () => {
    const root = declarationsFor(css, ':root');
    assert.equal(root.get('--radius-control'), `${RADII_PX.control}px`);
    assert.equal(root.get('--radius-panel'), `${RADII_PX.panel}px`);
    assert.equal(root.get('--radius-deck-key'), `${RADII_PX.deckKey}px`);
  });

  it('mirrors the focus ring geometry', () => {
    const root = declarationsFor(css, ':root');
    assert.equal(root.get('--focus-ring-width'), `${FOCUS_RING.widthPx}px`);
    assert.equal(
      root.get('--focus-ring-separation'),
      `${FOCUS_RING.separationPx}px`,
    );
  });

  it('mirrors motion budgets', () => {
    const root = declarationsFor(css, ':root');
    assert.equal(
      root.get('--motion-direct-manipulation-max'),
      `${MOTION.directManipulationMsMax}ms`,
    );
    assert.equal(
      root.get('--motion-panel-transition'),
      `${MOTION.panelTransitionMs}ms`,
    );
  });

  it('mirrors touch-target and deck-key minimums', () => {
    const root = declarationsFor(css, ':root');
    assert.equal(root.get('--size-target-min'), `${TARGETS.pointerTargetMinPx}px`);
    assert.equal(root.get('--size-deck-key-width'), `${TARGETS.deckKeyWidthPx}px`);
    assert.equal(root.get('--size-deck-key-height'), `${TARGETS.deckKeyHeightPx}px`);
  });
});

describe('accessibility rules in the stylesheets', () => {
  const globalCss = readUiFile('src/styles/global.css');
  const tokensCss = readUiFile('src/styles/tokens.css');

  it('style :focus-visible as a solid signal ring with canvas separation', () => {
    const focus = parseCssRules(globalCss).find(
      (rule) => rule.selector === ':focus-visible',
    );
    assert.ok(focus, ':focus-visible rule present');
    assert.equal(focus.declarations.get('outline'), 'var(--focus-ring-width) solid var(--signal)');
    assert.equal(focus.declarations.get('outline-offset'), 'var(--focus-ring-separation)');
  });

  it('zero all motion under prefers-reduced-motion', () => {
    const mediaRules = parseCssRules(tokensCss).filter(
      (rule) => rule.selector === ':root' && rule.declarations.has('--motion-panel-transition'),
    );
    const reduced = mediaRules.find(
      (rule) => rule.declarations.get('--motion-direct-manipulation-max') === '0ms',
    );
    assert.ok(reduced, 'reduced-motion override zeroes both budgets');
    assert.equal(reduced.declarations.get('--motion-panel-transition'), '0ms');
  });

  it('provide an explicit no-animation mode class', () => {
    const noMotion = parseCssRules(tokensCss).find((rule) =>
      rule.selector.startsWith('.no-motion'),
    );
    assert.ok(noMotion, '.no-motion escape hatch present');
  });

  it('keep body text at the tokenized default size', () => {
    const body = declarationsFor(globalCss, 'body');
    assert.equal(body.get('font-size'), 'var(--font-size-16)');
  });
});
