import type { ReactElement } from 'react';
import { createElement } from 'react';
import type { MessageCatalog } from './i18n/catalog.ts';

/**
 * M0 Studio shell view, built with plain element factories so the exact
 * markup is importable both by Vite (via App.tsx) and by the Node test
 * runner for the executable accessibility contract.
 *
 * Accessibility invariants encoded here and asserted by
 * src/testing/shell-a11y.test.ts: one h1 followed by an h2 inside a named
 * section, decorative shapes hidden from assistive technology, textual
 * status always present next to any color/shape indicator, DOM order equal
 * to reading order, and no manual tabindex.
 */
export function renderShell(messages: MessageCatalog): ReactElement {
  return createElement(
    'div',
    { className: 'shell' },
    createElement(
      'header',
      { className: 'shell-header' },
      createElement('h1', { className: 'shell-title' }, messages['app.title']),
      createElement(
        'p',
        { className: 'shell-tagline' },
        messages['app.tagline'],
      ),
    ),
    createElement(
      'main',
      { className: 'shell-main' },
      createElement(
        'section',
        { 'aria-labelledby': 'engine-status-heading', className: 'panel' },
        createElement(
          'h2',
          { id: 'engine-status-heading', className: 'panel-title' },
          messages['engine.heading'],
        ),
        createElement(
          'p',
          { className: 'status-line' },
          createElement('span', {
            'aria-hidden': 'true',
            className: 'status-dot',
          }),
          createElement(
            'span',
            { className: 'visually-hidden' },
            messages['engine.status.label'],
          ),
          messages['engine.status.notConnected'],
        ),
        createElement('p', { className: 'muted' }, messages['engine.body.muted']),
      ),
    ),
    createElement(
      'footer',
      { className: 'shell-footer' },
      createElement('p', { className: 'muted' }, messages['shell.footer.note']),
    ),
  );
}
