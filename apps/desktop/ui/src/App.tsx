import type { ReactElement } from 'react';
import { renderShell } from './app-view.ts';
import { messagesFor, DEFAULT_LOCALE } from './i18n/catalog.ts';

/**
 * M0 Studio shell.
 *
 * Renders an honest, accessible placeholder: no engine connection, deck
 * state, or control surface exists yet. Status is conveyed by text and shape,
 * never by color alone (DESIGN_SYSTEM.md). Markup lives in app-view.ts so
 * the accessibility contract can execute against it; strings come from the
 * localization catalog (en-US default, pt-BR shipped).
 */
export function App(): ReactElement {
  return renderShell(messagesFor(DEFAULT_LOCALE));
}
