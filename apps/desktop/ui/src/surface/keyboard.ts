/**
 * Keyboard layer for the live deck surface (issue #18).
 *
 * DESIGN_SYSTEM.md: "Drag/drop always has keyboard alternatives" and the
 * accessibility contract requires complete keyboard operation. Every
 * pointer gesture has a binding here producing IDENTICAL machine events
 * (parity asserted by tests):
 *
 * - Space down / up   press-begin / press-end (hold and repeat work by
 *                     keeping the key held — timing comes from the same
 *                     threshold logic as pointer holds);
 * - Enter             a complete press+release cycle in one step, for
 *                     momentary activation habits;
 * - Escape            cancels an armed destructive key.
 *
 * Pure functions only; App translates outcomes into dispatches and bridge
 * calls, calling `preventDefault()` whenever an outcome exists so native
 * button activation never double-fires behind an intentional binding.
 */

import type { InteractionContext, MachineEvent } from './machine.ts';

/** Plain descriptor of the DOM KeyboardEvent fields the bindings read. */
export interface SurfaceKeyInput {
  readonly kind: 'keydown' | 'keyup';
  readonly key: string;
}

/**
 * Resolves one raw key event against the focused deck key identified by
 * `controlId`. Returns the machine events to run, or null when the key is
 * not bound here (or the control cannot interact at all).
 */
export function handleSurfaceKey(
  controlId: string,
  context: InteractionContext,
  input: SurfaceKeyInput,
): readonly MachineEvent[] | null {
  if (!context.enabled || context.policy === null) {
    return null;
  }
  switch (input.key) {
    case ' ':
      return input.kind === 'keydown'
        ? [{ type: 'press-begin', controlId }]
        : [{ type: 'press-end', controlId }];
    case 'Enter':
      // One complete momentary cycle; holding Space expresses hold/repeat.
      return input.kind === 'keydown'
        ? [
            { type: 'press-begin', controlId },
            { type: 'press-end', controlId },
          ]
        : null;
    case 'Escape':
      return input.kind === 'keydown' ? [{ type: 'arm-cancel', controlId }] : null;
    default:
      return null;
  }
}
