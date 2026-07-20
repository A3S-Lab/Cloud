import { describe, expect, it } from 'vitest';
import { compactDigest } from './console-format';

describe('console formatting', () => {
  it('preserves the digest algorithm while exposing a useful hash prefix', () => {
    expect(compactDigest(`sha256:${'a'.repeat(64)}`)).toBe('sha256:aaaaaaaa');
    expect(compactDigest('identifier-without-a-digest')).toBe('identifi');
  });
});
