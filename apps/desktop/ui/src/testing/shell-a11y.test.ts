import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { renderShell } from '../app-view.ts';
import { CATALOG, LOCALES, messagesFor } from '../i18n/catalog.ts';
import { readUiFile } from './css.ts';

/**
 * Keyboard and screen-reader criteria for the current shell states,
 * executed against the exact markup the app ships (rendered through
 * react-dom/server from the same view module App.tsx uses).
 */

const markup = (locale: (typeof LOCALES)[number]): string =>
  renderToStaticMarkup(renderShell(messagesFor(locale)));

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
  it('uses DOM order as reading order: header, main, footer appear once in order', () => {
    const dom = markup('en-US');
    const header = dom.indexOf('<header');
    const main = dom.indexOf('<main');
    const footer = dom.indexOf('<footer');
    assert.ok(header >= 0 && main > header && footer > main);
    assert.equal(dom.split('<header').length - 1, 1);
    assert.equal(dom.split('<main').length - 1, 1);
    assert.equal(dom.split('<footer').length - 1, 1);
  });

  it('adds no manual tab-order overrides (focus follows DOM order)', () => {
    assert.doesNotMatch(markup('en-US'), /tabindex/i);
    assert.doesNotMatch(markup('pt-BR'), /tabindex/i);
  });

  it('introduces no drag-only interactions or color-only controls yet', () => {
    // The M0 shell has zero interactive elements; when controls arrive they
    // must satisfy the focus/touch-target/state tests. Asserting the empty
    // baseline keeps this criterion executable today.
    assert.ok(!/[<](button|a|input|select|textarea)[\s>]/.test(markup('en-US')));
  });
});

describe('shell screen-reader criteria', () => {
  for (const locale of LOCALES) {
    it(`renders one h1 followed by an h2 (${locale})`, () => {
      const dom = markup(locale);
      assert.equal(dom.split('<h1').length - 1, 1);
      const h1 = dom.indexOf('<h1');
      const h2 = dom.indexOf('<h2');
      assert.ok(h2 > h1);
      assert.match(dom, /<section aria-labelledby="engine-status-heading"/);
      assert.match(dom, /<h2 id="engine-status-heading"/);
    });

    it(`hides the decorative status dot from assistive technology (${locale})`, () => {
      const dom = markup(locale);
      assert.match(
        dom,
        /<span[^>]*aria-hidden="true"[^>]*class="status-dot"|<span[^>]*class="status-dot"[^>]*aria-hidden="true"/,
      );
    });

    it(`conveys engine state as text next to the indicator, never color alone (${locale})`, () => {
      const catalog = CATALOG[locale];
      assert.ok(catalog);
      const dom = markup(locale);
      assert.ok(dom.includes(catalog['engine.status.notConnected']));
      assert.ok(dom.includes(catalog['engine.status.label']));
      assert.ok(
        dom.includes('visually-hidden'),
        'textual status label present for non-visual users',
      );
    });

    it(`localizes every user-visible string from the catalog (${locale})`, () => {
      const catalog = CATALOG[locale];
      assert.ok(catalog);
      const dom = markup(locale);
      for (const value of Object.values(catalog)) {
        assert.ok(dom.includes(value), `catalog string rendered: ${value}`);
      }
    });
  }

  it('keeps the muted helper text out of the visual-hidden channel', () => {
    // Sanity: visually-hidden is used exactly once (the status label).
    const dom = markup('en-US');
    assert.equal(dom.split('visually-hidden').length - 1, 1);
  });
});
