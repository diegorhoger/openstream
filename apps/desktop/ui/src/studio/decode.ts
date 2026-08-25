/**
 * Fail-closed client-side decoding of workspace documents (issue #17).
 *
 * Mirrors the validation invariants of `crates/openstream-domain` so the
 * editor can (a) refuse malformed snapshots before rendering and
 * (b) give the inspector instant feedback. The Rust service remains the
 * AUTHORITATIVE gate — every op is re-validated there fail-closed; a
 * disagreement resolves toward refusal, never toward acceptance.
 *
 * Errors are closed-vocabulary tokens shared with the Rust side's
 * `StudioError::token()` spellings so one localization catalog covers both.
 */

import {
  CONTROL_KINDS,
  KIND_ALLOWS_POLICY,
  LIMITS,
  SUPPORTED_SCHEMA_VERSION,
  type Control,
  type Geometry,
  type InteractionPolicy,
  type Page,
} from './types.ts';

/** Canonical lowercase hyphenated UUIDv7 (version nibble `7`, RFC variant). */
const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export type ValidationToken = string;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function checkId(value: unknown, entity: string, errors: ValidationToken[]): void {
  if (typeof value !== 'string' || !UUID_V7.test(value)) {
    errors.push(`invalid_id:${entity}`);
  }
}

function checkText(field: string, value: unknown, errors: ValidationToken[]): void {
  if (typeof value !== 'string' || value.trim().length === 0 || value.length > LIMITS.maxTextBytes) {
    errors.push(`text_out_of_range:${field}`);
  }
}

/**
 * Folder-path grammar mirror (`FolderPath::parse`): segments non-empty, no
 * `.`/`..`, no padding whitespace, bounded bytes, no separators/controls.
 */
export function folderPathErrors(raw: unknown): ValidationToken[] {
  const errors: ValidationToken[] = [];
  if (typeof raw !== 'string') {
    return ['invalid_folder'];
  }
  if (raw.length > LIMITS.maxFolderPathBytes) {
    return ['invalid_folder'];
  }
  if (raw.length === 0) {
    return errors;
  }
  for (const segment of raw.split('/')) {
    if (segment.length === 0 || segment !== segment.trim() || segment.trim().length === 0) {
      errors.push('invalid_folder');
    } else if (segment === '.' || segment === '..') {
      errors.push('invalid_folder');
    } else if (segment.length > LIMITS.maxFolderSegmentBytes) {
      errors.push('invalid_folder');
    } else if (/[\u0000-\u001f\u007f-\u009f]/.test(segment)) {
      errors.push('invalid_folder');
    } else if (segment.includes('\\')) {
      // Backslash is forbidden inside segments (FolderPath::parse).
      errors.push('invalid_folder');
    }
  }
  return [...new Set(errors)];
}

function geometryErrors(geometry: unknown, page: Page): ValidationToken[] {
  if (
    !isRecord(geometry) ||
    typeof geometry.x !== 'number' ||
    typeof geometry.y !== 'number' ||
    typeof geometry.width !== 'number' ||
    typeof geometry.height !== 'number'
  ) {
    return ['domain_rejected'];
  }
  const rect = geometry as unknown as Geometry;
  const errors: ValidationToken[] = [];
  if (!Number.isInteger(rect.x) || !Number.isInteger(rect.y)) {
    errors.push('domain_rejected');
  }
  if (rect.width < 1 || rect.height < 1) {
    errors.push('zero_extent');
  }
  if (rect.x + rect.width > page.grid.columns) {
    errors.push('geometry_outside_grid:x');
  }
  if (rect.y + rect.height > page.grid.rows) {
    errors.push('geometry_outside_grid:y');
  }
  return errors;
}

function controlErrors(control: unknown, page: Page): ValidationToken[] {
  if (!isRecord(control)) {
    return ['domain_rejected'];
  }
  const errors: ValidationToken[] = [];
  checkId(control.id, 'control', errors);
  if (control.page_id !== page.id) {
    errors.push('not_found:control');
  }
  if (typeof control.kind !== 'string' || !CONTROL_KINDS.includes(control.kind as never)) {
    errors.push('domain_rejected');
    return errors;
  }
  checkText('label', control.label, errors);
  const kind = control.kind as Control['kind'];
  if (typeof control.enabled !== 'boolean') {
    errors.push('domain_rejected');
  }
  const policy = control.policy;
  if (policy === null || policy === undefined) {
    if (kind !== 'variable_display') {
      errors.push('policy_not_allowed');
    }
  } else {
    const allowed = KIND_ALLOWS_POLICY[kind];
    if (allowed === undefined || !allowed.includes(policy as InteractionPolicy)) {
      errors.push('policy_not_allowed');
    }
  }
  errors.push(...geometryErrors(control.geometry, page));
  return errors;
}

function pageErrors(page: unknown, deckId: string): { errors: ValidationToken[]; page: Page | null } {
  if (!isRecord(page)) {
    return { errors: ['domain_rejected'], page: null };
  }
  const errors: ValidationToken[] = [];
  checkId(page.id, 'page', errors);
  if (page.deck_id !== deckId) {
    errors.push('not_found:page');
  }
  const grid = page.grid;
  if (
    !isRecord(grid) ||
    typeof grid.columns !== 'number' ||
    typeof grid.rows !== 'number' ||
    grid.columns < 1 ||
    grid.rows < 1
  ) {
    errors.push('zero_extent');
    return { errors, page: null };
  }
  if (!Array.isArray(page.controls)) {
    errors.push('domain_rejected');
    return { errors, page: null };
  }
  if (page.controls.length > LIMITS.maxControlsPerPage) {
    errors.push('limit_exceeded');
  }
  const seenIds = new Set<string>();
  for (const control of page.controls) {
    if (isRecord(control) && typeof control.id === 'string') {
      if (seenIds.has(control.id)) {
        errors.push('duplicate_control');
      }
      seenIds.add(control.id);
    }
    errors.push(...controlErrors(control, page as unknown as Page));
  }
  return { errors, page: page as unknown as Page };
}

/**
 * Validates a deck document against the v1 invariants.
 *
 * @returns error tokens; an empty array means the document is acceptable.
 */
export function validateDeckDocument(document: unknown): ValidationToken[] {
  if (!isRecord(document) || !isRecord(document.deck)) {
    return ['domain_rejected'];
  }
  const errors: ValidationToken[] = [];
  const version = document.schema_version;
  if (
    !isRecord(version) ||
    version.major !== SUPPORTED_SCHEMA_VERSION.major ||
    version.minor !== SUPPORTED_SCHEMA_VERSION.minor
  ) {
    errors.push('unknown_schema_version');
  }
  const deck = document.deck;
  checkId(deck.id, 'deck', errors);
  checkId(deck.workspace_id, 'workspace', errors);
  checkText('title', deck.title, errors);
  if (typeof deck.revision !== 'number' || !Number.isInteger(deck.revision) || deck.revision < 0) {
    errors.push('domain_rejected');
  }
  errors.push(...folderPathErrors(deck.folder_path));
  if (!Array.isArray(deck.pages)) {
    errors.push('domain_rejected');
    return errors;
  }
  if (deck.pages.length > LIMITS.maxPagesPerDeck) {
    errors.push('limit_exceeded');
  }
  const ordinals = new Set<number>();
  for (const rawPage of deck.pages) {
    if (isRecord(rawPage) && typeof rawPage.ordinal === 'number') {
      if (ordinals.has(rawPage.ordinal)) {
        errors.push('ordinal_conflict');
      }
      ordinals.add(rawPage.ordinal);
    }
    errors.push(...pageErrors(rawPage, String(deck.id ?? '')) .errors);
  }
  return [...new Set(errors)];
}

/**
 * Validates a profile document against the v1 invariants.
 */
export function validateProfileDocument(document: unknown): ValidationToken[] {
  if (!isRecord(document) || !isRecord(document.profile)) {
    return ['domain_rejected'];
  }
  const errors: ValidationToken[] = [];
  const version = document.schema_version;
  if (
    !isRecord(version) ||
    version.major !== SUPPORTED_SCHEMA_VERSION.major ||
    version.minor !== SUPPORTED_SCHEMA_VERSION.minor
  ) {
    errors.push('unknown_schema_version');
  }
  const profile = document.profile;
  checkId(profile.id, 'profile', errors);
  checkId(profile.workspace_id, 'workspace', errors);
  checkText('name', profile.name, errors);
  if (!Array.isArray(profile.deck_ids)) {
    errors.push('domain_rejected');
    return errors;
  }
  if (profile.deck_ids.length > LIMITS.maxDecksPerProfile) {
    errors.push('limit_exceeded');
  }
  const seen = new Set<string>();
  for (const id of profile.deck_ids) {
    checkId(id, 'deck', errors);
    if (typeof id === 'string') {
      if (seen.has(id)) {
        errors.push('duplicate_deck_ref');
      }
      seen.add(id);
    }
  }
  return [...new Set(errors)];
}
