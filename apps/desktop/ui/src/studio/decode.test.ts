import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import {
  folderPathErrors,
  validateDeckDocument,
  validateProfileDocument,
} from './decode.ts';
import type { DeckDocument } from './types.ts';

/**
 * Decoder contract (issue #17): the client mirror must accept exactly the
 * documents the Rust domain accepts and refuse the ones it refuses. The
 * golden fixtures are the SAME files the Rust crate's
 * `crates/openstream-domain/tests/golden_fixtures.rs` pins byte-for-byte,
 * so this suite doubles as cross-language serialization parity evidence.
 */

const REPO_FIXTURES = new URL(
  '../../../../../crates/openstream-domain/tests/fixtures/',
  import.meta.url,
);

function readFixture(name: string): string {
  return readFileSync(new URL(name, REPO_FIXTURES), 'utf8');
}

function mutate(raw: string, apply: (value: Record<string, unknown>) => void): string {
  const value = JSON.parse(raw) as Record<string, unknown>;
  apply(value);
  return JSON.stringify(value);
}

describe('golden fixtures from the Rust domain crate', () => {
  const deckJson = readFixture('deck-document-v1.json');
  const profileJson = readFixture('profile-document-v1.json');

  it('accepts the Rust deck document fixture byte-for-byte', () => {
    assert.deepEqual(validateDeckDocument(JSON.parse(deckJson)), []);
  });

  it('re-serializes the deck fixture to identical bytes (key order parity)', () => {
    const reparsed = JSON.parse(deckJson) as unknown;
    assert.equal(JSON.stringify(reparsed), deckJson);
  });

  it('accepts the Rust profile document fixture', () => {
    assert.deepEqual(validateProfileDocument(JSON.parse(profileJson)), []);
  });

  it('re-serializes the profile fixture to identical bytes', () => {
    const reparsed = JSON.parse(profileJson) as unknown;
    assert.equal(JSON.stringify(reparsed), profileJson);
  });
});

describe('deck document rejection matrix', () => {
  const raw = readFixture('deck-document-v1.json');
  const valid = JSON.parse(raw) as DeckDocument;

  const cases: Array<[string, (value: Record<string, unknown>) => void, string]> = [
    [
      'unknown schema minor',
      (v) => {
        v.schema_version = { major: 1, minor: 1 };
      },
      'unknown_schema_version',
    ],
    [
      'foreign schema major',
      (v) => {
        v.schema_version = { major: 2, minor: 0 };
      },
      'unknown_schema_version',
    ],
    [
      'non-v7 deck id',
      (v) => {
        const deck = v.deck as Record<string, unknown>;
        deck.id = '3b241101-e2bb-4255-8caf-4136c566a962';
      },
      'invalid_id:deck',
    ],
    [
      'empty title',
      (v) => {
        const deck = v.deck as Record<string, unknown>;
        deck.title = '   ';
      },
      'text_out_of_range:title',
    ],
    [
      'oversized title (256-byte ceiling)',
      (v) => {
        const deck = v.deck as Record<string, unknown>;
        deck.title = 'x'.repeat(257);
      },
      'text_out_of_range:title',
    ],
    [
      'folder path with empty segment',
      (v) => {
        const deck = v.deck as Record<string, unknown>;
        deck.folder_path = 'bad//path';
      },
      'invalid_folder',
    ],
    [
      'folder path with dot segment',
      (v) => {
        const deck = v.deck as Record<string, unknown>;
        deck.folder_path = 'a/../b';
      },
      'invalid_folder',
    ],
    [
      'zero grid column',
      (v) => {
        const pages = (v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>;
        pages[0]!.grid = { columns: 0, rows: 4 };
      },
      'zero_extent',
    ],
    [
      'control outside grid on x',
      (v) => {
        const controls = ((v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!
          .controls as Array<Record<string, unknown>>;
        controls[0]!.geometry = { x: 15, y: 0, width: 2, height: 1 };
      },
      'geometry_outside_grid:x',
    ],
    [
      'control outside grid on y',
      (v) => {
        const controls = ((v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!
          .controls as Array<Record<string, unknown>>;
        controls[0]!.geometry = { x: 0, y: 8, width: 1, height: 1 };
      },
      'geometry_outside_grid:y',
    ],
    [
      'zero control extent',
      (v) => {
        const controls = ((v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!
          .controls as Array<Record<string, unknown>>;
        controls[0]!.geometry = { x: 0, y: 0, width: 0, height: 1 };
      },
      'zero_extent',
    ],
    [
      'duplicate control ids',
      (v) => {
        const controls = ((v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!
          .controls as Array<Record<string, unknown>>;
        controls[1]!.id = controls[0]!.id;
      },
      'duplicate_control',
    ],
    [
      'state sink carrying an interaction policy',
      (v) => {
        const controls = ((v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!
          .controls as Array<Record<string, unknown>>;
        controls[3]!.policy = 'press';
      },
      'policy_not_allowed',
    ],
    [
      'toggle carrying hold policy',
      (v) => {
        const controls = ((v.deck as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!
          .controls as Array<Record<string, unknown>>;
        controls[1]!.policy = 'hold';
      },
      'policy_not_allowed',
    ],
    [
      'page referencing a foreign deck',
      (v) => {
        const deck = v.deck as Record<string, unknown>;
        const pages = deck.pages as Array<Record<string, unknown>>;
        pages[0]!.deck_id = '018f6a1c-7b21-7001-9f31-000000000099';
      },
      'not_found:page',
    ],
  ];

  for (const [name, apply, expectedToken] of cases) {
    it(`rejects: ${name}`, () => {
      const errors = validateDeckDocument(JSON.parse(mutate(raw, apply)));
      assert.ok(errors.includes(expectedToken), `expected ${expectedToken}, got ${JSON.stringify(errors)}`);
    });
  }

  it('rejects duplicate page ordinals', () => {
    const mutated = mutate(raw, (value) => {
      const deck = value.deck as Record<string, unknown>;
      const pages = deck.pages as Array<Record<string, unknown>>;
      pages[1]!.ordinal = pages[0]!.ordinal;
    });
    assert.ok(validateDeckDocument(JSON.parse(mutated)).includes('ordinal_conflict'));
  });

  it('keeps the pristine fixture acceptable after all mutations above', () => {
    assert.deepEqual(validateDeckDocument(valid), []);
  });
});

describe('profile document rejection matrix', () => {
  const raw = readFixture('profile-document-v1.json');

  it('rejects duplicate deck references', () => {
    const mutated = mutate(raw, (value) => {
      const profile = value.profile as Record<string, unknown>;
      const deckIds = profile.deck_ids as string[];
      deckIds.push(deckIds[0]!);
    });
    assert.ok(validateProfileDocument(JSON.parse(mutated)).includes('duplicate_deck_ref'));
  });

  it('rejects oversized names', () => {
    const mutated = mutate(raw, (value) => {
      const profile = value.profile as Record<string, unknown>;
      profile.name = 'x'.repeat(257);
    });
    assert.ok(validateProfileDocument(JSON.parse(mutated)).includes('text_out_of_range:name'));
  });

  it('rejects non-v7 deck references', () => {
    const mutated = mutate(raw, (value) => {
      const profile = value.profile as Record<string, unknown>;
      const deckIds = profile.deck_ids as string[];
      deckIds[0] = 'not-a-uuid';
    });
    assert.ok(validateProfileDocument(JSON.parse(mutated)).includes('invalid_id:deck'));
  });
});

describe('folder path grammar mirror', () => {
  it('accepts root and normal segments', () => {
    assert.deepEqual(folderPathErrors(''), []);
    assert.deepEqual(folderPathErrors('live/scene'), []);
  });

  it('rejects structural violations with one stable token', () => {
    for (const bad of ['//', 'a//b', '/a', 'a/', '.', '..', 'a/./b', ' a', 'a ', 'a\\b']) {
      assert.deepEqual(folderPathErrors(bad), ['invalid_folder'], `input ${bad}`);
    }
  });
});
