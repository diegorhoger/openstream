# Portability Bundles (issue #20)

Versioned `.openstream` import/export bundles for whole-workspace snapshots.

## 1. Design Goals

| Goal | Guarantee |
|------|-----------|
| Fail-closed import | Framing magic, container version, member count/name/size caps, closed-vocabulary names (structural path-traversal defense), decompression ratio guards, manifest/member bijection with SHA-256 verification, domain schema decoding, and workspace semantics all reject before anything is returned. Malformed or hostile input never yields partial content. |
| Exact round-trip | Building is deterministic: canonical member order, stored (uncompressed) members only, compact declaration-order JSON. Export → import → export is byte-identical on one build. Across builds the documented rule is semantic identity of decoded documents (§8). |
| No secret material | Bundles carry `DeckDocument` / `ProfileDocument` values only. `SecretValue` cannot serialize at all (TM-LOG-01), so vault-backed secrets are structurally absent from every byte a bundle can contain. |

## 2. Bundle Structure

A v1 `.openstream` file is a binary frame:

```text
[u8; 8]   magic "OSTRBNDL"
[u32 LE]  container format version (1)
[u32 LE]  member count
member*:
  [u32 LE] name_len | name bytes (closed vocabulary)
  [u32 LE] raw_len  (uncompressed size in bytes)
  [u8]     compression (0 = stored, 1 = deflate)
  [u32 LE] stored_len (bytes that follow)
  payload[stored_len]
```

## 3. Closed-Vocabulary Member Names

The v1 bundle contains exactly three kinds of members:

| Name Pattern | Description |
|---|---|
| `manifest.json` | The bundle manifest (always first) |
| `deck/<uuidv7>.json` | One deck document |
| `profile/<uuidv7>.json` | One profile document |

There is no free-form path surface. Every name is validated against this closed grammar before any byte of its payload is interpreted. Separators other than `/`, parent components (`..`), absolute forms, drive letters, backslashes, uppercase spellings, and non-ASCII lookalikes all reject as `IllegalMemberName`.

## 4. Manifest Schema

```json
{
  "schema_version": { "major": 1, "minor": 0 },
  "counts": { "decks": 1, "profiles": 1 },
  "entries": [
    { "name": "deck/<uuid>.json", "sha256": "<64 lowercase hex>" }
  ]
}
```

- `schema_version`: Fail-closed on foreign major or newer minor.
- `counts`: Must agree exactly with the number of deck/profile entries.
- `entries`: Canonically sorted by name, unique, with lowercase hex SHA-256 digests.

## 5. Size and Shape Limits (v1)

| Constant | Value | Description |
|---|---|---|
| `MAX_BUNDLE_FILE_BYTES` | 64 MiB | Maximum serialized bundle size |
| `MAX_MEMBER_COUNT` | 2048 | Maximum framed members (including manifest) |
| `MAX_MEMBER_NAME_BYTES` | 128 | Maximum member name length |
| `MAX_MEMBER_UNCOMPRESSED_BYTES` | 64 MiB | Maximum uncompressed member size |
| `MAX_BUNDLE_UNCOMPRESSED_BYTES` | 128 MiB | Maximum total uncompressed size |
| `MAX_DECOMPRESSION_RATIO` | 100 | Maximum `raw_len / stored_len` for deflated members |
| `RATIO_GUARD_FLOOR_BYTES` | 4 KiB | Below this, ratio check is skipped |

Every bound is enforced **before** the corresponding allocation or decompression.

## 6. Validation Layers

The parser applies defenses in this order:

1. File size cap
2. Magic bytes (`OSTRBNDL`)
3. Container version (must be 1)
4. Member count cap
5. Per-member name length and closed-vocabulary grammar
6. Per-member raw size cap
7. Decompression ratio guard (for deflated members)
8. Exact-length verification (stored payload matches `raw_len`)
9. Trailing-byte rejection
10. Manifest JSON decode and schema-version check
11. Manifest/member bijection with per-member SHA-256 hash verification
12. Domain document decode and save-time validation
13. Workspace semantics: uniform workspace ownership, deck-reference closure, switch-rule conflict freedom

Nothing is returned on any failure. A `ParsedBundle` guarantees documents that are individually valid, mutually coherent, and byte-exact against their recorded hashes.

## 7. Error Types

All failures are typed `BundleError` values:

| Variant | Meaning |
|---|---|
| `TooLarge` | Exceeded a declared size cap |
| `InvalidMagic` | Container does not start with `OSTRBNDL` |
| `UnsupportedContainerVersion` | Framing version not supported |
| `MalformedFrame` | Truncated, self-inconsistent, or trailing bytes |
| `IllegalMemberName` | Outside closed vocabulary |
| `DuplicateMember` | Two members share one name |
| `CompressionRatioExceeded` | Deflated member exceeds ratio cap |
| `ManifestDecode` | Bad JSON, unknown field, wrong types |
| `UnsupportedManifestVersion` | Foreign major or newer minor |
| `ManifestInconsistent` | Counts wrong, entries unsorted, bijection violated |
| `HashMismatch` | SHA-256 digest differs from manifest record |
| `Document` | Domain decode or validation failure |
| `WorkspaceMismatch` | Documents span multiple workspaces |
| `MissingDeckReference` | Profile references a deck outside the bundle |
| `ConflictingSwitchRules` | Cross-profile switch-trigger conflict |
| `IoFailed` | Filesystem IO failed |

## 8. Round-Trip Rule

**On one build:** export → import → export is byte-identical.

**Across builds:** semantic identity of decoded documents. The manifest version, member names, and document JSON structure are pinned by the domain model's versioning policy (DOMAIN_MODEL.md §1). Minor JSON whitespace changes across Rust versions do not affect decoded values.

## 9. Durable File Write

`write_bundle_file(path, bytes)` follows the same discipline as persistence migrations:

1. Write to a sibling `.tmp` file.
2. `sync_all()` before any rename.
3. Rotate the existing file to `.prev` (rather than destroying it first).
4. Rename `.tmp` to the destination.
5. Clean up `.prev` on success.

Crash windows leave either the old file intact, or the new file plus a recoverable `.prev` — never a half-written backup.

## 10. Atomic Restore

Restore is the documented atomic procedure over `WorkspaceStore::rewrite_all`:

1. Parse and fully validate the bundle (`parse_bundle`).
2. Replace the workspace inside a single SQLite transaction (`rewrite_all`).

Any earlier failure leaves the previous workspace state untouched. The integration tests in `tests/roundtrip.rs` prove this contract.

## 11. Secret Redaction

Bundles carry only `DeckDocument` and `ProfileDocument` values. Neither type has secret-bearing fields:

- `SecretValue` cannot serialize at all (TM-LOG-01).
- The raw-byte scan tests in `tests/secret_redaction.rs` prove no secret markers appear in serialized bundle output.

## 12. Migration and Rollback

### Forward Migration (v1 → v2)

When a future version introduces bundle manifest v1.1 or v2.0:

1. **Minor addition (1.0 → 1.1):** New optional fields in manifest or new member types. Old bundles remain importable; new bundles are rejected by old clients with `UnsupportedManifestVersion`.
2. **Major change (1.x → 2.0):** New framing format or incompatible manifest schema. Old clients reject with `UnsupportedContainerVersion` or `UnsupportedManifestVersion`. Migration tooling reads v1 bundles and writes v2 bundles.

### Rollback

Bundles are snapshots. Rollback is:

1. Export a bundle from the target version.
2. Import into the older version (must be format-compatible).
3. If the bundle uses features from a newer domain schema, import will reject with `BundleError::Document`.

The `.prev` rotation in `write_bundle_file` provides filesystem-level rollback: if a restore corrupts the workspace, the previous backup is at `<path>.prev`.

### Version Policy

Per DOMAIN_MODEL.md §1 and ADR-0005:

- **Tightening** limits (reducing caps) requires a bundle major bump with migration docs.
- **Loosening** limits (increasing caps) is an additive minor.
- Unknown manifest versions always reject (fail closed).
