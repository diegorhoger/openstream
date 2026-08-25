/**
 * Shell bridge for the Studio editor (issue #17).
 *
 * The port keeps the editor testable and the shell dependency explicit:
 * views never touch Tauri directly. The production realization invokes the
 * four application-defined commands (`studio_load` / `studio_apply` /
 * `studio_undo` / `studio_redo`) through the global `__TAURI__` object
 * (`withGlobalTauri`), which exists only inside the desktop shell — a plain
 * browser dev session reports the bridge unavailable honestly instead of
 * pretending to persist.
 */

import type { ApplyOutcome, LoadResult, StudioOp } from './types.ts';

export interface StudioBridge {
  load(): Promise<LoadResult>;
  apply(op: StudioOp): Promise<ApplyOutcome>;
  undo(): Promise<ApplyOutcome>;
  redo(): Promise<ApplyOutcome>;
}

/** Typed shape of the minimal slice of `window.__TAURI__` we use. */
interface TauriGlobal {
  core?: {
    invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  };
}

function tauriGlobal(): TauriGlobal | null {
  const candidate = (globalThis as Record<string, unknown>).__TAURI__;
  if (typeof candidate === 'object' && candidate !== null) {
    return candidate as TauriGlobal;
  }
  return null;
}

/** True when running inside the desktop shell with an invokable bridge. */
export function bridgeAvailable(): boolean {
  return typeof tauriGlobal()?.core?.invoke === 'function';
}

/** Closed-vocabulary token used when no shell bridge exists. */
export const BRIDGE_UNAVAILABLE_TOKEN = 'bridge-unavailable';

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invokeFn = tauriGlobal()?.core?.invoke;
  if (typeof invokeFn !== 'function') {
    return Promise.reject(new Error(BRIDGE_UNAVAILABLE_TOKEN));
  }
  return invokeFn(command, args) as Promise<T>;
}

/** Production bridge over the desktop shell's IPC commands. */
export const tauriBridge: StudioBridge = {
  async load(): Promise<LoadResult> {
    return invoke<LoadResult>('studio_load');
  },
  apply(op: StudioOp): Promise<ApplyOutcome> {
    return invoke<ApplyOutcome>('studio_apply', { op });
  },
  undo(): Promise<ApplyOutcome> {
    return invoke<ApplyOutcome>('studio_undo');
  },
  redo(): Promise<ApplyOutcome> {
    return invoke<ApplyOutcome>('studio_redo');
  },
};

/**
 * Scripted in-memory bridge for component tests: records every command so
 * tests can assert exactly which ops each interaction path produces.
 */
export class FakeStudioBridge implements StudioBridge {
  readonly appliedOps: StudioOp[] = [];
  private snapshotState: LoadResult;
  private outcomeOverride: ((op: StudioOp) => ApplyOutcome) | null = null;

  constructor(initial: LoadResult = {
    snapshot: { decks: [], profiles: [] },
    autosave_active: true,
  }) {
    this.snapshotState = initial;
  }

  /** Replaces the outcome factory for subsequent applies. */
  onApply(factory: (op: StudioOp) => ApplyOutcome): void {
    this.outcomeOverride = factory;
  }

  load(): Promise<LoadResult> {
    return Promise.resolve(this.snapshotState);
  }

  apply(op: StudioOp): Promise<ApplyOutcome> {
    this.appliedOps.push(op);
    if (this.outcomeOverride !== null) {
      return Promise.resolve(this.outcomeOverride(op));
    }
    return Promise.resolve(this.snapshotState as unknown as ApplyOutcome);
  }

  undo(): Promise<ApplyOutcome> {
    throw new Error('undo not scripted');
  }

  redo(): Promise<ApplyOutcome> {
    throw new Error('redo not scripted');
  }
}
