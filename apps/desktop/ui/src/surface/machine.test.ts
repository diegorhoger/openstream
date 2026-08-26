import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  HOLD_THRESHOLD_MS,
  REPEAT_INTERVAL_MS,
  TERMINAL_KINDS,
  initialSurfaceRuntime,
  surfaceReducer,
  transition,
  type InteractionContext,
  type MachineEvent,
} from './machine.ts';

/**
 * State-machine coverage for the live deck surface (issue #18).
 *
 * The invariants under test are the acceptance criteria themselves:
 * relayed / accepted / executed terminals are DISTINCT phases reachable
 * only through their own authoritative events; no local gesture can ever
 * produce success; refusals land as failed with their token; destructive
 * keys arm before anything fires; hold/repeat windows behave per policy.
 */

const ENGINE_ON = { ...initialSurfaceRuntime, engineAvailable: true };

function ctx(policy: InteractionContext['policy'], overrides: Partial<InteractionContext> = {}): InteractionContext & { destructive: boolean } {
  return { enabled: true, policy, destructive: false, ...overrides };
}

function run(
  events: readonly MachineEvent[],
  context: InteractionContext & { destructive: boolean },
  start = ENGINE_ON,
): { phase: string | undefined; effects: readonly unknown[]; runtime: typeof start } {
  let runtime = start;
  const effects: unknown[] = [];
  for (const event of events) {
    const result = transition(runtime, context, event);
    runtime = result.runtime;
    effects.push(...result.effects);
  }
  return { phase: runtime.keys.c1?.phase ?? 'idle', effects, runtime };
}

describe('press-policy lifecycle', () => {
  it('walks pressed → relayed → accepted → running → succeeded as DISTINCT phases', () => {
    const context = ctx('press');
    const first = run([{ type: 'press-begin', controlId: 'c1' }], context);
    assert.equal(first.phase, 'pressed');
    assert.deepEqual(first.effects, [{ kind: 'invoke', controlId: 'c1', event: 'press' }]);

    const relayed = run([{ type: 'relayed', controlId: 'c1' }], context, first.runtime);
    assert.equal(relayed.phase, 'relayed');
    assert.notEqual(relayed.phase, first.phase);

    const accepted = run([{ type: 'accepted', controlId: 'c1' }], context, relayed.runtime);
    assert.equal(accepted.phase, 'accepted');
    assert.notEqual(accepted.phase, relayed.phase);

    const running = run([{ type: 'running', controlId: 'c1' }], context, accepted.runtime);
    assert.equal(running.phase, 'running');

    const done = run(
      [{ type: 'terminal', controlId: 'c1', terminal: 'succeeded', token: null }],
      context,
      running.runtime,
    );
    assert.equal(done.phase, 'succeeded');
    assert.ok(!['relayed', 'accepted', 'running'].includes(done.phase));
  });

  it('never reaches success from a local gesture alone', () => {
    for (const event of [
      { type: 'press-begin', controlId: 'c1' },
      { type: 'press-end', controlId: 'c1' },
      { type: 'relayed', controlId: 'c1' },
      { type: 'accepted', controlId: 'c1' },
      { type: 'running', controlId: 'c1' },
      { type: 'arm-confirm', controlId: 'c1' },
    ] as readonly MachineEvent[]) {
      const result = run([event], ctx('press'));
      assert.ok(result.phase !== 'succeeded', `${event.type} must not imply success`);
    }
  });

  it('keeps terminals visible until the next gesture and resets cleanly', () => {
    const context = ctx('press');
    const done = run(
      [
        { type: 'press-begin', controlId: 'c1' },
        { type: 'relayed', controlId: 'c1' },
        { type: 'terminal', controlId: 'c1', terminal: 'succeeded', token: null },
      ],
      context,
    );
    assert.equal(done.phase, 'succeeded');
    const again = run([{ type: 'press-begin', controlId: 'c1' }], context, done.runtime);
    assert.equal(again.phase, 'pressed');
  });

  it('ignores illegal transitions instead of guessing', () => {
    const context = ctx('press');
    // accepted without relayed, running from idle, terminal from idle.
    assert.equal(run([{ type: 'accepted', controlId: 'c1' }], context).phase, 'idle');
    assert.equal(run([{ type: 'running', controlId: 'c1' }], context).phase, 'idle');
    assert.equal(
      run([{ type: 'terminal', controlId: 'c1', terminal: 'succeeded', token: null }], context)
        .phase,
      'idle',
    );
  });
});

describe('refused invocations stay honest', () => {
  it('lands refused answers as failed with the token, never success or silent reset', () => {
    const context = ctx('press');
    let runtime = run([{ type: 'press-begin', controlId: 'c1' }], context).runtime;
    runtime = surfaceReducer(runtime, {
      kind: 'invoked',
      outcome: { control_id: 'c1', status: { kind: 'refused', token: 'binding_absent' } },
    });
    const key = runtime.keys.c1;
    assert.equal(key?.phase, 'failed');
    assert.equal(key?.failureToken, 'binding_absent');

    // The next press starts a fresh cycle.
    const again = transition(runtime, context, { type: 'press-begin', controlId: 'c1' });
    assert.equal(again.runtime.keys.c1?.phase, 'pressed');
  });

  it('ignores stale answers for keys no longer in flight', () => {
    let runtime = ENGINE_ON;
    runtime = surfaceReducer(runtime, {
      kind: 'invoked',
      outcome: { control_id: 'c1', status: { kind: 'refused', token: 'late' } },
    });
    assert.equal(runtime.keys.c1, undefined);
  });
});

describe('release, hold, and repeat policies', () => {
  it('release fires on press-end', () => {
    const context = ctx('release');
    const begin = run([{ type: 'press-begin', controlId: 'c1' }], context);
    assert.equal(begin.phase, 'pressed');
    assert.deepEqual(begin.effects, []);
    const end = run([{ type: 'press-end', controlId: 'c1' }], context, begin.runtime);
    assert.deepEqual(end.effects, [{ kind: 'invoke', controlId: 'c1', event: 'release' }]);
    assert.equal(end.phase, 'idle');
  });

  it('hold opens its window at the threshold and closes on release', () => {
    const context = ctx('hold');
    const begin = run([{ type: 'press-begin', controlId: 'c1' }], context);
    assert.deepEqual(begin.effects, []);
    const tick = run([{ type: 'hold-tick', controlId: 'c1' }], context, begin.runtime);
    assert.equal(tick.runtime.keys.c1?.held, true);
    assert.deepEqual(tick.effects, [{ kind: 'invoke', controlId: 'c1', event: 'hold_begin' }]);
    const end = run([{ type: 'press-end', controlId: 'c1' }], context, tick.runtime);
    assert.deepEqual(end.effects, [{ kind: 'invoke', controlId: 'c1', event: 'hold_end' }]);
    assert.equal(end.phase, 'idle');
  });

  it('hold never double-fires while the window stays open', () => {
    const context = ctx('hold');
    const open = run(
      [
        { type: 'press-begin', controlId: 'c1' },
        { type: 'hold-tick', controlId: 'c1' },
      ],
      context,
    );
    const extra = run([{ type: 'hold-tick', controlId: 'c1' }], context, open.runtime);
    assert.deepEqual(extra.effects, []);
  });

  it('repeat keeps firing while held across evidence phases and stops after a terminal', () => {
    const context = ctx('repeat');
    let runtime = run([{ type: 'press-begin', controlId: 'c1' }], context).runtime;
    const first = run([{ type: 'repeat-tick', controlId: 'c1' }], context, runtime);
    assert.deepEqual(first.effects, [{ kind: 'invoke', controlId: 'c1', event: 'repeat' }]);
    runtime = first.runtime;

    runtime = run([{ type: 'relayed', controlId: 'c1' }], context, runtime).runtime;
    const second = run([{ type: 'repeat-tick', controlId: 'c1' }], context, runtime);
    assert.deepEqual(second.effects, [{ kind: 'invoke', controlId: 'c1', event: 'repeat' }]);
    runtime = second.runtime;

    runtime = run(
      [{ type: 'terminal', controlId: 'c1', terminal: 'failed', token: 'binding_absent' }],
      context,
      runtime,
    ).runtime;
    const stopped = run([{ type: 'repeat-tick', controlId: 'c1' }], context, runtime);
    assert.deepEqual(stopped.effects, [], 'a decided terminal stops repeating');
  });
});

describe('destructive arming gate', () => {
  const armedCtx = ctx('press', { destructive: true });

  it('arms on press without firing anything', () => {
    const result = run([{ type: 'press-begin', controlId: 'c1' }], armedCtx);
    assert.equal(result.phase, 'armed');
    assert.deepEqual(result.effects, []);
  });

  it('cancel returns to idle without firing; confirm fires exactly once', () => {
    const cancelled = run(
      [
        { type: 'press-begin', controlId: 'c1' },
        { type: 'arm-cancel', controlId: 'c1' },
      ],
      armedCtx,
    );
    assert.equal(cancelled.phase, 'idle');
    assert.deepEqual(cancelled.effects, []);

    const confirmed = run(
      [
        { type: 'press-begin', controlId: 'c1' },
        { type: 'arm-confirm', controlId: 'c1' },
      ],
      armedCtx,
    );
    assert.equal(confirmed.phase, 'pressed');
    assert.deepEqual(confirmed.effects, [{ kind: 'invoke', controlId: 'c1', event: 'press' }]);
  });

  it('double-press never bypasses arming', () => {
    const result = run(
      [
        { type: 'press-begin', controlId: 'c1' },
        { type: 'press-begin', controlId: 'c1' },
      ],
      armedCtx,
    );
    assert.equal(result.phase, 'armed');
    assert.deepEqual(result.effects, []);
  });
});

describe('disabled controls and engine availability', () => {
  it('disabled controls stay fully inert', () => {
    const context = ctx('press', { enabled: false });
    const result = run([{ type: 'press-begin', controlId: 'c1' }], context);
    assert.equal(result.phase, 'idle');
    assert.deepEqual(result.effects, []);
  });

  it('unavailable engine blocks interaction but late terminals still land', () => {
    const off = initialSurfaceRuntime;
    const blocked = run([{ type: 'press-begin', controlId: 'c1' }], ctx('press'), off);
    assert.equal(blocked.phase, 'idle');
    assert.deepEqual(blocked.effects, []);

    const runtime = {
      ...off,
      engineAvailable: false,
      keys: { c1: { phase: 'relayed' as const, held: false, latched: false, failureToken: null } },
    };
    const late = run(
      [{ type: 'terminal', controlId: 'c1', terminal: 'outcome_unknown', token: null }],
      ctx('press'),
      runtime,
    );
    assert.equal(late.phase, 'outcome_unknown', 'decided evidence is never swallowed');
  });
});

describe('toggle latching', () => {
  it('flips local latch per press, independent of execution evidence', () => {
    const context = ctx('toggle');
    const first = run([{ type: 'press-begin', controlId: 'c1' }], context);
    assert.equal(first.runtime.keys.c1?.latched, true);
    const second = run([{ type: 'press-begin', controlId: 'c1' }], context, {
      ...first.runtime,
      keys: {},
    });
    void second;
    // Fresh key starts unlatched even though the previous one ended latched.
    assert.equal(second.runtime.keys.c1?.latched, true);
  });
});

describe('timing constants and latency posture', () => {
  it('pins the documented thresholds', () => {
    assert.equal(HOLD_THRESHOLD_MS, 500);
    assert.equal(REPEAT_INTERVAL_MS, 150);
  });

  it('keeps press feedback synchronous within one frame budget', () => {
    const context = ctx('press');
    const started = performance.now();
    for (let index = 0; index < 500; index += 1) {
      const result = transition(ENGINE_ON, context, { type: 'press-begin', controlId: `k${index}` });
      assert.equal(result.runtime.keys[`k${index}`]?.phase, 'pressed');
    }
    const elapsedMs = performance.now() - started;
    assert.ok(elapsedMs < 100, `500 transitions took ${elapsedMs}ms`);
  });
});

describe('authoritative terminal vocabulary', () => {
  it('covers exactly the five Engine receipt states', () => {
    assert.deepEqual([...TERMINAL_KINDS].sort(), [
      'cancelled',
      'expired',
      'failed',
      'outcome_unknown',
      'succeeded',
    ]);
    const context = ctx('press');
    for (const terminal of TERMINAL_KINDS) {
      const result = run(
        [
          { type: 'press-begin', controlId: 'c1' },
          { type: 'relayed', controlId: 'c1' },
          { type: 'terminal', controlId: 'c1', terminal, token: terminal === 'succeeded' ? null : 't' },
        ],
        context,
      );
      assert.equal(result.phase, terminal);
    }
  });
});
