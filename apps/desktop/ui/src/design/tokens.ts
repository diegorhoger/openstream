import type { HexColor } from './color.ts';

/**
 * OpenStream design tokens — executable source of truth.
 *
 * Authority: docs/design/DESIGN_SYSTEM.md. This module is the versioned,
 * typed, code-consumable form of that document; `styles/tokens.css` mirrors
 * it as CSS custom properties and a parity test keeps both in lockstep
 * (src/testing/css-parity.test.ts).
 *
 * Versioning follows semantic versioning for the exported shape: additive
 * tokens are a minor bump, removal/rename/value break is a major bump.
 */

export const TOKENS_VERSION = '1.0.0';

/** Semantic color vocabulary from DESIGN_SYSTEM.md "Color tokens". */
export const COLOR_TOKEN_NAMES = [
  'canvas',
  'surface-1',
  'surface-2',
  'border',
  'text',
  'text-muted',
  'signal',
  'live',
  'warning',
  'info',
] as const;

export type ColorTokenName = (typeof COLOR_TOKEN_NAMES)[number];

export type ThemeColors = Readonly<Record<ColorTokenName, HexColor>>;

/** Dark theme (default). Values are verbatim from DESIGN_SYSTEM.md. */
export const DARK_COLORS: ThemeColors = {
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
};

/** Light theme override. Values are verbatim from DESIGN_SYSTEM.md. */
export const LIGHT_COLORS: ThemeColors = {
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
};

export const THEMES = { dark: DARK_COLORS, light: LIGHT_COLORS } as const;
export type ThemeId = keyof typeof THEMES;
export const THEME_IDS: readonly ThemeId[] = ['dark', 'light'];

/** UI font scale in px (DESIGN_SYSTEM.md "Typography"). */
export const TYPOGRAPHY_SCALE_PX: readonly number[] = [12, 14, 16, 20, 24, 32, 40];
export const BODY_FONT_SIZE_PX = 16;
export const MIN_TEXT_SIZE_PX = 12;
export const MONO_FONT_STACK =
  "'IBM Plex Mono', ui-monospace, 'Cascadia Mono', 'SF Mono', Consolas, monospace";
export const UI_FONT_STACK =
  "system-ui, -apple-system, 'Segoe UI', Roboto, Inter, sans-serif";

/** Geometry scale in px (DESIGN_SYSTEM.md "Geometry"). */
export const SPACING_BASE_PX = 4;
export const SPACING_RHYTHM_PX = 8;
export const SPACING_STEPS_PX: readonly number[] = [4, 8, 12, 16, 24, 32, 48];

export const RADII_PX = {
  control: 6,
  panel: 10,
  deckKey: 14,
} as const;

/** Focus ring: 3 px signal with 2 px canvas separation. */
export const FOCUS_RING = {
  widthPx: 3,
  separationPx: 2,
} as const;

/** Motion budgets in ms (DESIGN_SYSTEM.md "Motion and sound"). */
export const MOTION = {
  directManipulationMsMin: 80,
  directManipulationMsMax: 160,
  panelTransitionMs: 200,
} as const;

/** Minimum interactive geometry in CSS px (DESIGN_SYSTEM.md "Geometry"). */
export const TARGETS = {
  pointerTargetMinPx: 44,
  deckKeyWidthPx: 88,
  deckKeyHeightPx: 88,
} as const;

/**
 * Control state vocabulary, verbatim from DESIGN_SYSTEM.md
 * "Control states". Every control can render each of these states.
 */
export const CONTROL_STATES = [
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
] as const;

export type ControlState = (typeof CONTROL_STATES)[number];

/**
 * Delivery states are separate from visual control states: "relayed",
 * "accepted", and "executed" must never be conflated with succeeded/running
 * (DESIGN_SYSTEM.md: "Relayed," "accepted," and "executed" are separate
 * states; SECURITY.md: no success before the authoritative Engine result).
 */
export const DELIVERY_STATES = ['relayed', 'accepted', 'executed'] as const;
export type DeliveryState = (typeof DELIVERY_STATES)[number];

/**
 * Status is never conveyed by color alone: every state pairs its color with
 * an icon, label, shape, or motion-independent indicator.
 */
export const NON_COLOR_INDICATOR_RULE =
  'Every state indicator combines color with an icon, label, shape, or motion-independent text so status remains legible without color perception.';
