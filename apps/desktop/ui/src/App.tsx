import type { ReactElement } from 'react';

/**
 * M0 Studio shell.
 *
 * Renders an honest, accessible placeholder: no engine connection, deck
 * state, or control surface exists yet. Status is conveyed by text and shape,
 * never by color alone (DESIGN_SYSTEM.md).
 */
export function App(): ReactElement {
  return (
    <div className="shell">
      <header className="shell-header">
        <h1 className="shell-title">OpenStream</h1>
        <p className="shell-tagline">The open control surface for live production.</p>
      </header>
      <main className="shell-main">
        <section aria-labelledby="engine-status-heading" className="panel">
          <h2 id="engine-status-heading" className="panel-title">
            Engine
          </h2>
          <p className="status-line">
            <span aria-hidden="true" className="status-dot" />
            <span className="visually-hidden">Engine status: </span>
            Not connected
          </p>
          <p className="muted">
            The local Engine is not wired into this shell yet. Deck editing and device
            control arrive in later milestones.
          </p>
        </section>
      </main>
      <footer className="shell-footer">
        <p className="muted">M0 scaffold &mdash; no account, no network surface.</p>
      </footer>
    </div>
  );
}
