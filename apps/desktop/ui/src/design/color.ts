/**
 * Branded hex color type shared by design tokens and contrast tests.
 * A plain string at runtime; `HexColor` documents the #RRGGBB shape.
 */
export type HexColor = string;

export function isHexColor(value: string): value is HexColor {
  return /^#[0-9a-fA-F]{6}$/.test(value);
}

function srgbChannelToLinear(channel: number): number {
  const s = channel / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/** WCAG 2.x relative luminance of an sRGB hex color. */
export function relativeLuminance(hex: HexColor): number {
  if (!isHexColor(hex)) {
    throw new RangeError(`not a #RRGGBB color: ${hex}`);
  }
  const r = srgbChannelToLinear(Number.parseInt(hex.slice(1, 3), 16));
  const g = srgbChannelToLinear(Number.parseInt(hex.slice(3, 5), 16));
  const b = srgbChannelToLinear(Number.parseInt(hex.slice(5, 7), 16));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * WCAG 2.x contrast ratio between two sRGB colors, from 1 (identical) to
 * 21 (black on white).
 */
export function contrastRatio(a: HexColor, b: HexColor): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const [lighter, darker] = la >= lb ? [la, lb] : [lb, la];
  return (lighter + 0.05) / (darker + 0.05);
}
