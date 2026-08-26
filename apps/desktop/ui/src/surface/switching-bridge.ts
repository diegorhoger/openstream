/**
 * Shell bridge for the switching panel (issue #19).
 *
 * Same discipline as the other bridges: views never touch Tauri directly;
 * the production realization invokes exactly two application-defined
 * commands (`switch_state_load` / `switch_consent`).
 */

import {
  CONSENT_ACTIONS,
  type ConsentAction,
  type SwitchLoadResult,
} from './switching-types.ts';

export interface SwitchingBridge {
  load(): Promise<SwitchLoadResult>;
  consent(action: ConsentAction): Promise<SwitchLoadResult>;
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
export function switchingBridgeAvailable(): boolean {
  return typeof tauriGlobal()?.core?.invoke === 'function';
}

async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
  const invokeFn = tauriGlobal()?.core?.invoke;
  if (typeof invokeFn !== 'function') {
    throw new Error('bridge-unavailable');
  }
  return invokeFn(command, args);
}

/** Production bridge over the desktop shell's IPC commands. */
export const tauriSwitchingBridge: SwitchingBridge = {
  async load(): Promise<SwitchLoadResult> {
    return (await invoke('switch_state_load')) as SwitchLoadResult;
  },
  async consent(action: ConsentAction): Promise<SwitchLoadResult> {
    if (!CONSENT_ACTIONS.includes(action)) {
      throw new Error(`unknown-consent-action:${action}`);
    }
    return (await invoke('switch_consent', { action })) as SwitchLoadResult;
  },
};

/**
 * Scripted in-memory bridge for component tests: serves a fixed state and
 * records every consent action in order.
 */
export class FakeSwitchingBridge implements SwitchingBridge {
  readonly consents: ConsentAction[] = [];
  /** Overrides what every load returns after tests arrange failures. */
  failNextLoad = false;

  constructor(private state: SwitchLoadResult) {}

  setState(state: SwitchLoadResult): void {
    this.state = state;
  }

  load(): Promise<SwitchLoadResult> {
    if (this.failNextLoad) {
      this.failNextLoad = false;
      return Promise.reject(new Error('bridge-unavailable'));
    }
    return Promise.resolve(this.state);
  }

  consent(action: ConsentAction): Promise<SwitchLoadResult> {
    this.consents.push(action);
    return Promise.resolve(this.state);
  }
}
