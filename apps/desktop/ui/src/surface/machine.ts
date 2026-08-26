/**
 * Pure execution-state machine for the live deck surface (issue #18).
 *
 * The machine owns ONLY presentation truth for each deck key, derived
 * exclusively from explicit events: local interaction (press/release/hold/
 * repeat), transport evidence (`relayed`), and AUTHORITATIVE Engine
 * outcomes (`accepted`, `running`, terminals). There is no path from a
 * local gesture to `succeeded` — success exists only as an authoritative
 * event, so failed or unaccepted actions can never present as done
 * (SECURITY.md hard rule; DOMAIN_MODEL.md §4 "states are derived from
 * Engine journal evidence only").
 *
 * `relayed`, `accepted`, and the executed terminals are DISTINCT phases by
 * construction: each is reachable only through its own event and the view
 * renders each with its own icon/text/color pairing.
 *
 * Phase semantics per policy (DOMAIN_MODEL.md §4):
 * - press / toggle: one invocation on press; the phase cycle runs
 *   pressed → relayed → (accepted → running) → terminal, and the terminal
 *   stays visible until the next gesture;
 * - release: invocation fires on release; the phase cycle runs entirely
 *   after release and its terminal stays visible;
 * - hold / repeat: the key stays `pressed` (with `held`) for the whole
 *   gesture while invocations fire; transport/execution evidence advances
 *   through the SAME distinct phases between fires, and release ends the
 *   gesture honestly.
 *
 * Destructive arming: destructive controls enter `armed` on press and stay
 * there until an explicit confirm (which then runs the normal press flow)
 * or cancel. This milestone has no destructive bindings anywhere (no
 * binding vocabulary exists yet), so production passes an EMPTY destructive
 * set; tests drive the flow synthetically so the gate is proven before any
 * destructive binding can ever ship.
 *
 * Toggle latching is control-local presentation (`latched`), kept strictly
 * separate from every execution phase: it never claims an action result.
 */

import type { InteractionEvent } from './types.ts';

/**
 * Every renderable phase of one deck key. Pre-execution phases:
 * idle/pressed/armed. Transport and execution evidence phases: relayed,
 * accepted, running, and the five authoritative terminals.
 */
export type ExecutionPhase =
  | 'idle'
  | 'pressed'
  | 'armed'
  | 'relayed'
  | 'accepted'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'expired'
  | 'outcome_unknown';

/** Authoritative terminal kinds (Engine receipt vocabulary). */
export type TerminalKind =
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'expired'
  | 'outcome_unknown';

export const TERMINAL_KINDS: readonly TerminalKind[] = [
  'succeeded',
  'failed',
  'cancelled',
  'expired',
  'outcome_unknown',
];

/** Presentation truth of one deck key. */
export interface KeyRuntime {
  readonly phase: ExecutionPhase;
  /** True from the moment a hold/repeat window opens until release. */
  readonly held: boolean;
  /** Control-local toggle latch; NEVER an action result. */
  readonly latched: boolean;
  /** Closed-vocabulary refusal/failure token when present. */
  readonly failureToken: string | null;
}

/** Machine-wide runtime keyed by control id. */
export interface SurfaceRuntime {
  readonly keys: Readonly<Record<string, KeyRuntime>>;
  /** Whether the local Engine composition is available behind the shell. */
  readonly engineAvailable: boolean;
}

export const INITIAL_KEY_RUNTIME: KeyRuntime = {
  phase: 'idle',
  held: false,
  latched: false,
  failureToken: null,
};

export const initialSurfaceRuntime: SurfaceRuntime = {
  keys: {},
  engineAvailable: false,
};

/** Everything the machine needs to know about one interaction target. */
export interface InteractionContext {
  /** Whether the control is user-enabled. */
  readonly enabled: boolean;
  /** Resolved interaction policy, or null for state sinks. */
  readonly policy: 'press' | 'release' | 'hold' | 'repeat' | 'toggle' | null;
  /** Whether this binding is destructive-class (arming required). */
  readonly destructive: boolean;
}

/** Events the pure transition function consumes. */
export type MachineEvent =
  | { readonly type: 'press-begin'; readonly controlId: string }
  | { readonly type: 'press-end'; readonly controlId: string }
  /** Hold threshold reached while held (caller owns real time). */
  | { readonly type: 'hold-tick'; readonly controlId: string }
  /** Repeat interval elapsed while held (caller owns real time). */
  | { readonly type: 'repeat-tick'; readonly controlId: string }
  | { readonly type: 'arm-confirm'; readonly controlId: string }
  | { readonly type: 'arm-cancel'; readonly controlId: string }
  /** Transport evidence: the command left toward the Engine. */
  | { readonly type: 'relayed'; readonly controlId: string }
  /** Authoritative admission evidence (durable accepted record). */
  | { readonly type: 'accepted'; readonly controlId: string }
  /** Authoritative running evidence. */
  | { readonly type: 'running'; readonly controlId: string }
  /** Authoritative terminal evidence. */
  | {
      readonly type: 'terminal';
      readonly controlId: string;
      readonly terminal: TerminalKind;
      readonly token: string | null;
    };

/** Side effects the caller MUST perform for the transition to be honest. */
export type MachineEffect =
  | { readonly kind: 'invoke'; readonly controlId: string; readonly event: InteractionEvent };

/** Result of one pure transition step. */
export interface TransitionResult {
  readonly runtime: SurfaceRuntime;
  readonly effects: readonly MachineEffect[];
}

function keyOf(runtime: SurfaceRuntime, controlId: string): KeyRuntime {
  return runtime.keys[controlId] ?? INITIAL_KEY_RUNTIME;
}

function withKey(
  runtime: SurfaceRuntime,
  controlId: string,
  key: KeyRuntime,
): SurfaceRuntime {
  return { ...runtime, keys: { ...runtime.keys, [controlId]: key } };
}

const IN_FLIGHT_PHASES: readonly ExecutionPhase[] = [
  'pressed',
  'relayed',
  'accepted',
  'running',
];

/**
 * Which wire event one policy's press-step maps to.
 */
export function gestureEventFor(
  policy: NonNullable<InteractionContext['policy']>,
): InteractionEvent {
  switch (policy) {
    case 'press':
    case 'toggle':
      return 'press';
    case 'release':
      return 'release';
    case 'hold':
      return 'hold_begin';
    case 'repeat':
      return 'repeat';
  }
}

/** Hold threshold (ms) after which hold/repeat windows open. */
export const HOLD_THRESHOLD_MS = 500;

/** Repeat interval (ms) between repeating fires while held. */
export const REPEAT_INTERVAL_MS = 150;

/**
 * Pure transition function. Never mutates inputs; returns the next runtime
 * plus the effects that MUST run. Illegal transitions are ignored (the
 * runtime stays put) rather than guessed.
 *
 * Invariants enforced here and asserted by tests:
 * - `succeeded`/`failed`/`cancelled`/`expired`/`outcome_unknown` are
 *   reachable ONLY through the authoritative `terminal` event;
 * - `relayed` only through `relayed`; `accepted` only through `accepted`;
 * - refused invocations land as `failed` carrying their token — never
 *   silently reset, never success;
 * - destructive keys require arm-confirm before any press flow begins.
 */
export function transition(
  runtime: SurfaceRuntime,
  context: InteractionContext,
  event: MachineEvent,
): TransitionResult {
  // Engine availability gates every INTERACTIVE event globally; late
  // authoritative terminals always land so dropped connections can never
  // swallow already-decided evidence.
  if (!runtime.engineAvailable && event.type !== 'terminal') {
    return { runtime, effects: [] };
  }

  switch (event.type) {
    case 'press-begin': {
      const key = keyOf(runtime, event.controlId);
      if (!context.enabled || context.policy === null) {
        return { runtime, effects: [] };
      }
      if (key.phase === 'armed' || key.phase === 'pressed') {
        return { runtime, effects: [] };
      }
      if (context.destructive) {
        return {
          runtime: withKey(runtime, event.controlId, {
            ...key,
            phase: 'armed',
            held: false,
            failureToken: null,
          }),
          effects: [],
        };
      }
      const latched = context.policy === 'toggle' ? !key.latched : key.latched;
      const next = withKey(runtime, event.controlId, {
        ...key,
        phase: 'pressed',
        held: false,
        latched,
        failureToken: null,
      });
      const effects: MachineEffect[] = [];
      if (context.policy === 'press' || context.policy === 'toggle') {
        effects.push({
          kind: 'invoke',
          controlId: event.controlId,
          event: gestureEventFor(context.policy),
        });
      }
      return { runtime: next, effects };
    }
    case 'arm-confirm': {
      const key = keyOf(runtime, event.controlId);
      if (!context.enabled || context.policy === null || key.phase !== 'armed') {
        return { runtime, effects: [] };
      }
      // Confirmation completes the armed press: one invocation fires with
      // the policy's own wire event, exactly as a plain press would. The
      // key stays pressed so transport/execution evidence remains visible
      // until the terminal lands or the gesture ends.
      return {
        runtime: withKey(runtime, event.controlId, {
          ...key,
          phase: 'pressed',
          held: false,
          failureToken: null,
        }),
        effects: [
          {
            kind: 'invoke',
            controlId: event.controlId,
            event: gestureEventFor(context.policy),
          },
        ],
      };
    }
    case 'arm-cancel': {
      const key = keyOf(runtime, event.controlId);
      if (key.phase !== 'armed') {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, {
          ...key,
          phase: 'idle',
          held: false,
          failureToken: null,
        }),
        effects: [],
      };
    }
    case 'press-end': {
      const key = keyOf(runtime, event.controlId);
      if (!context.enabled || context.policy === null) {
        return { runtime, effects: [] };
      }
      if (key.phase === 'armed') {
        // Arming resolves only through explicit confirm/cancel.
        return { runtime, effects: [] };
      }
      const effects: MachineEffect[] = [];
      if (context.policy === 'release') {
        effects.push({ kind: 'invoke', controlId: event.controlId, event: 'release' });
      } else if (context.policy === 'hold' && key.held) {
        effects.push({ kind: 'invoke', controlId: event.controlId, event: 'hold_end' });
      }
      return {
        runtime: withKey(runtime, event.controlId, { ...key, phase: 'idle', held: false }),
        effects,
      };
    }
    case 'hold-tick': {
      const key = keyOf(runtime, event.controlId);
      if (
        !context.enabled ||
        context.policy !== 'hold' ||
        key.phase !== 'pressed' ||
        key.held
      ) {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, { ...key, held: true }),
        effects: [{ kind: 'invoke', controlId: event.controlId, event: 'hold_begin' }],
      };
    }
    case 'repeat-tick': {
      const key = keyOf(runtime, event.controlId);
      if (
        !context.enabled ||
        context.policy !== 'repeat' ||
        !(key.phase === 'pressed' || IN_FLIGHT_PHASES.includes(key.phase)) ||
        (key.phase !== 'pressed' && !key.held)
      ) {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, { ...key, held: true }),
        effects: [{ kind: 'invoke', controlId: event.controlId, event: 'repeat' }],
      };
    }
    case 'relayed': {
      const key = keyOf(runtime, event.controlId);
      if (!IN_FLIGHT_PHASES.includes(key.phase)) {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, { ...key, phase: 'relayed' }),
        effects: [],
      };
    }
    case 'accepted': {
      const key = keyOf(runtime, event.controlId);
      if (key.phase !== 'relayed') {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, { ...key, phase: 'accepted' }),
        effects: [],
      };
    }
    case 'running': {
      const key = keyOf(runtime, event.controlId);
      if (key.phase !== 'relayed' && key.phase !== 'accepted') {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, { ...key, phase: 'running' }),
        effects: [],
      };
    }
    case 'terminal': {
      const key = keyOf(runtime, event.controlId);
      if (!IN_FLIGHT_PHASES.includes(key.phase)) {
        return { runtime, effects: [] };
      }
      return {
        runtime: withKey(runtime, event.controlId, {
          ...key,
          phase: event.terminal,
          held: false,
          failureToken:
            event.terminal === 'succeeded' ? null : (event.token ?? 'unknown_outcome'),
        }),
        effects: [],
      };
    }
  }
}

/** One dispatched app-level action for the surface reducer. */
export type SurfaceAction =
  | ({ readonly kind: 'machine' } & MachineEvent & {
      readonly context: InteractionContext;
    })
  | {
      readonly kind: 'invoked';
      /** Authoritative response to one invocation attempt. */
      readonly outcome: {
        readonly control_id: string;
        readonly status:
          | { readonly kind: 'refused'; readonly token: string }
          | { readonly kind: 'succeeded' };
      };
    }
  | { readonly kind: 'engine'; readonly available: boolean };

/**
 * Reducer over surface actions for React integration. Invocation responses
 * ALWAYS terminate the in-flight cycle authoritatively: refusals land as
 * `failed` carrying their token (never a silent reset, never success).
 */
export function surfaceReducer(
  state: SurfaceRuntime,
  action: SurfaceAction,
): SurfaceRuntime {
  switch (action.kind) {
    case 'engine':
      return { ...state, engineAvailable: action.available };
    case 'invoked': {
      const key = state.keys[action.outcome.control_id];
      if (key === undefined || !IN_FLIGHT_PHASES.includes(key.phase)) {
        return state;
      }
      if (action.outcome.status.kind === 'refused') {
        return withKey(state, action.outcome.control_id, {
          ...key,
          phase: 'failed',
          held: false,
          failureToken: action.outcome.status.token,
        });
      }
      return withKey(state, action.outcome.control_id, {
        ...key,
        phase: 'succeeded',
        held: false,
        failureToken: null,
      });
    }
    case 'machine': {
      const { context, ...event } = action;
      return transition(state, context, event).runtime;
    }
  }
}
