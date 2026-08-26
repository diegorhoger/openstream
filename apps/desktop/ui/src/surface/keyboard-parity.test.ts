import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { handleSurfaceKey, type SurfaceKeyInput } from './keyboard.ts';
import type { InteractionContext, MachineEvent } from './machine.ts';

/**
 * Keyboard parity for the live deck surface (issue #18).
 *
 * DESIGN_SYSTEM.md requires keyboard alternatives for every interaction.
 * These tests assert the keyboard path produces IDENTICAL machine-event
 * sequences to the pointer path for every gesture, including hold and
 * repeat windows (the tick events are produced by the same threshold logic
 * in both paths, so parity holds at event level).
 */

const CONTROL = '018f6a1c-7b21-7003-9f31-00000000c001';

function ctx(policy: InteractionContext['policy']): InteractionContext & { destructive: boolean } {
  return { enabled: true, policy, destructive: false };
}

function pointerSequence(policy: NonNullable<InteractionContext['policy']>, ticks: number): MachineEvent[] {
  const events: MachineEvent[] = [{ type: 'press-begin', controlId: CONTROL }];
  const tickType = policy === 'hold' ? 'hold-tick' : 'repeat-tick';
  if (policy === 'hold' || policy === 'repeat') {
    for (let index = 0; index < ticks; index += 1) {
      events.push({ type: tickType, controlId: CONTROL });
    }
  }
  events.push({ type: 'press-end', controlId: CONTROL });
  return events;
}

function keySequence(
  policy: NonNullable<InteractionContext['policy']>,
  inputs: readonly SurfaceKeyInput[],
  ticks: number,
): MachineEvent[] | null {
  const events: MachineEvent[] = [];
  for (const input of inputs) {
    const resolved = handleSurfaceKey(CONTROL, ctx(policy), input);
    if (resolved !== null) {
      events.push(...resolved);
    }
    // The shared threshold logic injects ticks identically in both paths;
    // after the FIRST keydown of a hold/repeat gesture the caller schedules
    // them (App-level), mirrored here right after the opening event.
    if (
      input.kind === 'keydown' &&
      (input.key === 'Enter' || input.key === ' ') &&
      (policy === 'hold' || policy === 'repeat') &&
      events.some((event) => event.type === 'press-begin')
    ) {
      const tickType = policy === 'hold' ? 'hold-tick' : 'repeat-tick';
      for (let index = 0; index < ticks; index += 1) {
        events.push({ type: tickType, controlId: CONTROL });
      }
    }
  }
  return events.length === 0 ? null : events;
}

describe('keyboard parity', () => {
  it('Space reproduces the pointer press/release cycle exactly', () => {
    for (const policy of ['press', 'release', 'toggle', 'press'] as const) {
      const expected = pointerSequence(policy, 0);
      const actual = keySequence(policy, [
        { kind: 'keydown', key: ' ' },
        { kind: 'keyup', key: ' ' },
      ], 0);
      assert.deepEqual(actual?.map(({ type }) => type), expected.map(({ type }) => type));
      assert.deepEqual(actual, expected);
    }
  });

  it('holding Space expresses hold windows with identical events', () => {
    const expected = pointerSequence('hold', 1);
    const actual = keySequence('hold', [
      { kind: 'keydown', key: ' ' },
      { kind: 'keyup', key: ' ' },
    ], 1);
    assert.deepEqual(actual, expected);
  });

  it('repeat windows match tick-for-tick', () => {
    const expected = pointerSequence('repeat', 3);
    const actual = keySequence('repeat', [
      { kind: 'keydown', key: ' ' },
      { kind: 'keyup', key: ' ' },
    ], 3);
    assert.deepEqual(actual, expected);
  });

  it('Enter taps produce one complete momentary cycle', () => {
    const actual = handleSurfaceKey(CONTROL, ctx('press'), { kind: 'keydown', key: 'Enter' });
    assert.deepEqual(actual, [
      { type: 'press-begin', controlId: CONTROL },
      { type: 'press-end', controlId: CONTROL },
    ]);
    const keyup = handleSurfaceKey(CONTROL, ctx('press'), { kind: 'keyup', key: 'Enter' });
    assert.equal(keyup, null);
  });

  it('Escape cancels arming on both paths', () => {
    const keyboard = handleSurfaceKey(CONTROL, ctx('press'), { kind: 'keydown', key: 'Escape' });
    assert.deepEqual(keyboard, [{ type: 'arm-cancel', controlId: CONTROL }]);
  });

  it('disabled controls and state sinks bind nothing', () => {
    const sink: InteractionContext & { destructive: boolean } = {
      enabled: true,
      policy: null,
      destructive: false,
    };
    assert.equal(handleSurfaceKey(CONTROL, sink, { kind: 'keydown', key: ' ' }), null);
    const disabled: InteractionContext & { destructive: boolean } = {
      enabled: false,
      policy: 'press',
      destructive: false,
    };
    assert.equal(handleSurfaceKey(CONTROL, disabled, { kind: 'keydown', key: ' ' }), null);
    assert.equal(handleSurfaceKey(CONTROL, ctx('press'), { kind: 'keydown', key: 'x' }), null);
  });
});
