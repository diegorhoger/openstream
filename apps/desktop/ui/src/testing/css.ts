import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export interface CssRule {
  selector: string;
  declarations: Map<string, string>;
}

function stripComments(css: string): string {
  return css.replaceAll(/\/\*[\s\S]*?\*\//g, '');
}

/**
 * Minimal brace-matching parser for the flat token stylesheets used by the
 * accessibility contract. Returns one entry per declaration block; blocks
 * inside at-rules (e.g. @media) are flattened with their inner selectors.
 */
export function parseCssRules(cssText: string): CssRule[] {
  const css = stripComments(cssText);
  const rules: CssRule[] = [];
  let i = 0;

  const readSelector = (): string => {
    const start = css.indexOf('{', i);
    if (start < 0) {
      i = css.length;
      return '';
    }
    const selector = css.slice(i, start).trim();
    i = start + 1;
    return selector;
  };

  while (i < css.length) {
    while (i < css.length && /\s/.test(css[i] ?? '')) {
      i += 1;
    }
    if (i >= css.length) {
      break;
    }
    if (css.startsWith('@media', i) || css.startsWith('@supports', i)) {
      // Skip the prelude; recurse into the block with flattened selectors.
      const open = css.indexOf('{', i);
      i = open + 1;
      const close = matchingBrace(css, open);
      rules.push(...parseCssRules(css.slice(i, close)));
      i = close + 1;
      continue;
    }
    if (css.startsWith('@keyframes', i) || css.startsWith('@charset', i)) {
      const open = css.indexOf('{', i);
      const close = matchingBrace(css, open);
      i = close + 1;
      continue;
    }
    const selector = readSelector();
    const close = matchingBrace(css, i - 1);
    const body = css.slice(i, close);
    i = close + 1;
    const declarations = new Map<string, string>();
    for (const declaration of body.split(';')) {
      const idx = declaration.indexOf(':');
      if (idx < 0) {
        continue;
      }
      const prop = declaration.slice(0, idx).trim();
      const value = declaration.slice(idx + 1).trim();
      if (prop.length > 0) {
        declarations.set(prop, value);
      }
    }
    rules.push({ selector, declarations });
  }
  return rules;
}

function matchingBrace(text: string, openIndex: number): number {
  let depth = 0;
  for (let j = openIndex; j < text.length; j += 1) {
    const ch = text[j];
    if (ch === '{') {
      depth += 1;
    } else if (ch === '}') {
      depth -= 1;
      if (depth === 0) {
        return j;
      }
    }
  }
  return text.length - 1;
}

/** Resolve this UI package's directory regardless of the caller's cwd. */
export function uiPackageRoot(): string {
  // src/testing/<file> -> package root is three levels up.
  return fileURLToPath(new URL('../../', import.meta.url));
}

export function readUiFile(relativePath: string): string {
  return readFileSync(uiPackageRoot() + relativePath, 'utf8');
}
