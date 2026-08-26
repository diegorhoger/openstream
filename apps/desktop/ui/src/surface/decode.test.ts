import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { invokeOutcomeErrors, surfaceLoadErrors } from './decode.ts';
import type { WorkspaceSnapshot } from '../studio/types.ts';

/**
 * Fail-closed decode coverage for the surface wire contract (issue #18).
 */

const VALID_SNAPSHOT: WorkspaceSnapshot = {
  decks: [
    {
      schema_version: { major: 1, minor: 0 },
      deck: {
        id: '018f6a1c-7b21-7001-9f31-0000000000a1',
        workspace_id: '018f6a1c-7b21-7000-9f31-000000000000',
        title: 'Live',
        revision: 0,
        folder_path: '',
        deleted_at: null,
        pages: [
          {
            id: '018f6a1c-7b21-7002-9f31-000000000001',
            deck_id: '018f6a1c-7b21-7001-9f31-0000000000a1',
            ordinal: 0,
            grid: { columns: 8, rows: 4 },
            controls: [],
          },
        ],
      },
    },
  ],
  profiles: [],
};

describe('surfaceLoadErrors', () => {
  it('accepts a well-formed load result', () => {
    assert.deepEqual(
      surfaceLoadErrors({ snapshot: VALID_SNAPSHOT, engine_available: true }),
      [],
    );
  });

  it('refuses non-objects and malformed snapshots before rendering', () => {
    assert.deepEqual(surfaceLoadErrors(null), ['surface_load_invalid']);
    assert.deepEqual(surfaceLoadErrors(42), ['surface_load_invalid']);
    assert.deepEqual(surfaceLoadErrors({ engine_available: true }), ['surface_load_invalid']);
    assert.deepEqual(
      surfaceLoadErrors({ snapshot: { decks: 'no' }, engine_available: true }),
      ['surface_load_invalid'],
    );
  });

  it('refuses invalid documents inside the snapshot (defense in depth)', () => {
    const broken = structuredClone(VALID_SNAPSHOT);
    const deck = broken.decks[0];
    if (deck === undefined) {
      assert.fail('fixture deck missing');
    }
    deck.deck.title = '   ';
    const errors = surfaceLoadErrors({ snapshot: broken, engine_available: true });
    assert.ok(errors.includes('text_out_of_range:title'), String(errors));
  });

  it('requires the honest boolean engine flag', () => {
    const errors = surfaceLoadErrors({ snapshot: VALID_SNAPSHOT, engine_available: 'yes' });
    assert.deepEqual(errors, ['surface_engine_flag_invalid']);
  });
});

describe('invokeOutcomeErrors', () => {
  const refused = {
    control_id: '018f6a1c-7b21-7003-9f31-00000000c001',
    interaction: 'press',
    status: { kind: 'refused', token: 'binding_absent' },
  };

  it('accepts refused outcomes — refusals are honest results, not failures', () => {
    assert.deepEqual(invokeOutcomeErrors(refused), []);
  });

  it('rejects structural violations', () => {
    assert.deepEqual(invokeOutcomeErrors(null), ['invoke_outcome_invalid']);
    assert.deepEqual(invokeOutcomeErrors(42), ['invoke_outcome_invalid']);
    assert.deepEqual(invokeOutcomeErrors({}), [
      'invoke_outcome_invalid',
      'invoke_interaction_unknown',
    ]);
    assert.deepEqual(
      invokeOutcomeErrors({ ...refused, interaction: 'detonate' }),
      ['invoke_interaction_unknown'],
    );
    assert.deepEqual(
      invokeOutcomeErrors({ ...refused, status: { kind: 'succeeded' } }),
      ['invoke_status_unknown'],
      'only the current closed vocabulary decodes; unknown kinds refuse',
    );
    assert.deepEqual(
      invokeOutcomeErrors({ ...refused, status: { kind: 'refused', token: '' } }),
      ['invoke_outcome_invalid'],
    );
  });
});
