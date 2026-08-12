import { afterEach, describe, expect, it, vi } from 'vitest';
import { compactDigest, formatRelative, formatTimestamp, statusBadgeState } from './console-format';

afterEach(() => {
  vi.restoreAllMocks();
});

describe('console formatting', () => {
  it('preserves the digest algorithm while exposing a useful hash prefix', () => {
    expect(compactDigest(`sha256:${'a'.repeat(64)}`)).toBe('sha256:aaaaaaaa');
    expect(compactDigest('identifier-without-a-digest')).toBe('identifi');
  });
});

describe('console date formatting', () => {
  it('keeps the English console language independent of the browser locale', () => {
    vi.spyOn(Date, 'now').mockReturnValue(Date.parse('2026-07-23T12:00:00Z'));

    expect(formatRelative('2026-07-22T02:00:00Z')).toMatch(/^[A-Z][a-z]{2} \d{1,2},/);
    expect(formatTimestamp('2026-07-22T02:00:00Z')).toMatch(/^[A-Z][a-z]{2} \d{1,2},/);
  });
});

describe('console Status Badge mapping', () => {
  it('projects domain statuses onto the reusable semantic state contract', () => {
    expect(['live', 'running', 'verifying', 'provisioning'].map(statusBadgeState)).toEqual([
      'active',
      'active',
      'active',
      'active',
    ]);
    expect(['succeeded', 'completed', 'ready', 'verified'].map(statusBadgeState)).toEqual([
      'success',
      'success',
      'success',
      'success',
    ]);
    expect(['connecting', 'retrying', 'cancelling', 'retiring', 'issued'].map(statusBadgeState)).toEqual([
      'warning',
      'warning',
      'warning',
      'warning',
      'warning',
    ]);
    expect(['failed', 'orphaned', 'rejected', 'revoked'].map(statusBadgeState)).toEqual([
      'danger',
      'danger',
      'danger',
      'danger',
    ]);
    expect(statusBadgeState('unknown-status')).toBe('neutral');
    expect(statusBadgeState('in-progress')).toBe('warning');
  });
});
