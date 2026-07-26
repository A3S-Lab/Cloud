import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';

const ENROLLMENT_TOKEN = `a3sn_${'a'.repeat(64)}`;

describe('CloudApi node enrollment credentials', () => {
  it('issues one short-lived credential through the existing tenant-scoped Fleet path', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ replayed: false }, 201);
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });

    await api.issueEnrollmentToken(
      'organization / one',
      {
        name: 'worker-1',
        token: ENROLLMENT_TOKEN,
        expiresAt: '2026-07-27T01:15:00Z',
      },
      'fleet:bootstrap:worker-1'
    );

    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe('/api/v1/organizations/organization%20%2F%20one/enrollment-tokens');
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Bearer caller-token',
          'Idempotency-Key': 'fleet:bootstrap:worker-1',
          'Content-Type': 'application/json',
        }),
        body: JSON.stringify({
          name: 'worker-1',
          token: ENROLLMENT_TOKEN,
          expiresAt: '2026-07-27T01:15:00Z',
        }),
      })
    );
  });

  it.each([
    ['a3sn_short', 'a3sn_ prefix followed by 64 lowercase hex digits'],
    [`a3sn_${'A'.repeat(64)}`, 'a3sn_ prefix followed by 64 lowercase hex digits'],
  ])('rejects an invalid enrollment credential before transport %#', (token, message) => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return envelope({});
      },
    });

    expect(() =>
      api.issueEnrollmentToken(
        'organization',
        { name: 'worker-1', token, expiresAt: '2026-07-27T01:15:00Z' },
        'fleet:bootstrap:worker-1'
      )
    ).toThrow(message);
    expect(called).toBe(false);
  });

  it('rejects an invalid enrollment expiry before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return envelope({});
      },
    });

    expect(() =>
      api.issueEnrollmentToken(
        'organization',
        { name: 'worker-1', token: ENROLLMENT_TOKEN, expiresAt: '2026-02-30T01:15:00Z' },
        'fleet:bootstrap:worker-1'
      )
    ).toThrow('enrollment credential expiry must be an RFC 3339 timestamp');
    expect(called).toBe(false);
  });
});

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000020',
      timestamp: '2026-07-27T00:00:00.000Z',
    }),
    { status }
  );
}
