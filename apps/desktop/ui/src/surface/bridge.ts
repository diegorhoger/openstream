/**
 * Shell bridge for the live deck surface (issue #18).
 *
 * Same discipline as the Studio bridge: views never touch Tauri directly;
 * the production realization invokes exactly two application-defined
 * commands (`surface_load` / `surface_invoke`) through the global
 * `__TAURI__` object, which exists only inside the desktop shell.
 */

import type { InvokeOutcome, SurfaceLoadResult } from './types.ts';

export interface SurfaceBridge {
  load(): Promise<SurfaceLoadResult>;
  invoke(controlId: string, interaction: string): Promise<InvokeOutcome>;
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
export function surfaceBridgeAvailable(): boolean {
  return typeof tauriGlobal()?.core?.invoke === 'function';
}

/** Closed-vocabulary token used when no shell bridge exists. */
export const SURFACE_BRIDGE_UNAVAILABLE_TOKEN = 'bridge-unavailable';

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invokeFn = tauriGlobal()?.core?.invoke;
  if (typeof invokeFn !== 'function') {
    return Promise.reject(new Error(SURFACE_BRIDGE_UNAVAILABLE_TOKEN));
  }
  return invokeFn(command, args) as Promise<T>;
}

/** Production bridge over the desktop shell's IPC commands. */
export const tauriSurfaceBridge: SurfaceBridge = {
  async load(): Promise<SurfaceLoadResult> {
    return invoke<SurfaceLoadResult>('surface_load');
  },
  invoke(controlId: string, interaction: string): Promise<InvokeOutcome> {
    return invoke<InvokeOutcome>('surface_invoke', {
      controlId,
      interaction,
    });
  },
};

/**
 * Scripted in-memory bridge for component tests: records every invocation
 * so tests can assert exactly what each interaction path produced, and can
 * script responses (or transport failures) per call.
 */
export class FakeSurfaceBridge implements SurfaceBridge {
  readonly invocations: { controlId: string; interaction: string }[] = [];
  private responder:
    | ((controlId: string, interaction: string) => Promise<InvokeOutcome>)
    | null = null;

  constructor(
    private readonly loadResult: SurfaceLoadResult = {
      snapshot: { decks: [], profiles: [] },
      engine_available: true,
    },
  ) {}

  /** Replaces the outcome factory for subsequent invocations. */
  respondWith(factory: (controlId: string, interaction: string) => Promise<InvokeOutcome>): void {
    this.responder = factory;
  }

  load(): Promise<SurfaceLoadResult> {
    return Promise.resolve(this.loadResult);
  }

  invoke(controlId: string, interaction: string): Promise<InvokeOutcome> {
    this.invocations.push({ controlId, interaction });
    if (this.responder !== null) {
      return this.responder(controlId, interaction);
    }
    return Promise.resolve({
      control_id: controlId,
      interaction,
      status: { kind: 'refused', token: 'binding_absent' },
    });
  }
}
