import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { contrastRatio, relativeLuminance } from '../design/color.ts';
import { THEMES, THEME_IDS, type ThemeColors } from '../design/tokens.ts';

/**
 * Executable contrast minimums (WCAG 2.2 AA; primary text targets AAA).
 *
 * Scope note, measured against the DESIGN_SYSTEM.md palette:
 * - `text` and `text-muted` meet AA on every background in both themes and
 *   primary `text` meets AAA everywhere.
 * - status colors (`signal`, `live`, `warning`, `info`) are text-capable on
 *   `canvas` and `surface-1` (AA) and always pass the 3:1 non-text
 *   indicator floor on all surfaces including `surface-2`.
 * - `border` is decorative reinforcement only (1.3–1.8:1); component
 *   identity never relies on it alone.
 */
const TEXT_TOKENS = ['text', 'text-muted'] as const;
const STATUS_TOKENS = ['signal', 'live', 'warning', 'info'] as const;
const SURFACES = ['canvas', 'surface-1', 'surface-2'] as const;

describe('wcag contrast math', () => {
  it('reproduces the canonical 21:1 black-on-white anchor', () => {
    assert.ok(Math.abs(contrastRatio('#000000', '#FFFFFF') - 21) < 1e-9);
  });

  it('is symmetric', () => {
    assert.ok(
      Math.abs(contrastRatio('#FF5D5D', '#14181A') - contrastRatio('#14181A', '#FF5D5D')) <
        1e-12,
    );
  });

  it('yields 1 for identical colors', () => {
    assert.ok(Math.abs(contrastRatio('#2DE2B4', '#2DE2B4') - 1) < 1e-12);
  });

  it('never increases when the foreground lightens on a white surface', () => {
    let previous = contrastRatio('#000000', '#FFFFFF');
    for (let c = 0; c <= 255; c += 15) {
      const hex = `#${c.toString(16).padStart(2, '0').repeat(3)}`;
      const current = contrastRatio(hex, '#FFFFFF');
      assert.ok(current <= previous + 1e-9);
      previous = current;
    }
  });

  it('keeps relative luminance inside [0, 1]', () => {
    for (const hex of ['#FFFFFF', '#000000', '#0B0D0E', '#F5F7F8', '#087D65']) {
      const l = relativeLuminance(hex);
      assert.ok(l >= 0 && l <= 1);
    }
  });
});

function assertMinimum(
  theme: string,
  colors: ThemeColors,
  foreground: string,
  background: string,
  minimum: number,
): void {
  const ratio = contrastRatio(colors[foreground as keyof ThemeColors], colors[background as keyof ThemeColors]);
  assert.ok(
    ratio >= minimum,
    `${theme}: ${foreground} on ${background} is ${ratio.toFixed(2)}:1, needs ${minimum}:1`,
  );
}

for (const theme of THEME_IDS) {
  describe(`contrast — ${theme} theme`, () => {
    const colors = THEMES[theme];

    it('meets AA (4.5:1) for text tokens on every surface', () => {
      for (const fg of TEXT_TOKENS) {
        for (const bg of SURFACES) {
          assertMinimum(theme, colors, fg, bg, 4.5);
        }
      }
    });

    it('meets AAA (7:1) for primary text on every surface', () => {
      for (const bg of SURFACES) {
        assertMinimum(theme, colors, 'text', bg, 7);
      }
    });

    it('makes status colors AA text-capable on canvas and panel surfaces', () => {
      for (const fg of STATUS_TOKENS) {
        for (const bg of ['canvas', 'surface-1'] as const) {
          assertMinimum(theme, colors, fg, bg, 4.5);
        }
      }
    });

    it('keeps status colors above the 3:1 non-text indicator floor on all surfaces', () => {
      for (const fg of STATUS_TOKENS) {
        for (const bg of SURFACES) {
          assertMinimum(theme, colors, fg, bg, 3);
        }
      }
    });

    it('gives the focus ring at least 3:1 against every surface', () => {
      for (const bg of SURFACES) {
        assertMinimum(theme, colors, 'signal', bg, 3);
      }
    });
  });
}
