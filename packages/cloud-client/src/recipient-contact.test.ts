import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';

function jsonResponse(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-08-23T00:00:00.000Z',
    }),
    { status: 200, headers: { 'content-type': 'application/json' } }
  );
}

describe('recipient contact client surface', () => {
  it('uses the five exact self-service routes with bounded JSON mutations', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const requestInput = {
      address: 'Operator+Alerts@Example.test',
      principalId: 'must-not-cross-the-client-boundary',
    };
    const proof = 'a3srcv1.opaque_payload.opaque_authenticator';
    const completionInput = { proof, privateMetadata: 'must-not-cross-the-client-boundary' };

    await api.listRecipientContacts('organization / one');
    await api.getRecipientContact('organization / one', 'contact / one');
    await api.requestRecipientContactVerification('organization / one', requestInput, 'recipient:request');
    await api.completeRecipientContactVerification(
      'organization / one',
      'contact / one',
      completionInput,
      'recipient:verify'
    );
    await api.revokeRecipientContact('organization / one', 'contact / one', 2, 'recipient:revoke');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/recipient-contacts',
      '/api/v1/organizations/organization%20%2F%20one/recipient-contacts/contact%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/recipient-contacts',
      '/api/v1/organizations/organization%20%2F%20one/recipient-contacts/contact%20%2F%20one/verification',
      '/api/v1/organizations/organization%20%2F%20one/recipient-contacts/contact%20%2F%20one/revocation',
    ]);
    expect(calls.slice(2).map(([, init]) => init)).toEqual([
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'recipient:request' }),
        body: JSON.stringify({ address: 'Operator+Alerts@Example.test' }),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'recipient:verify' }),
        body: JSON.stringify({ proof }),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'recipient:revoke' }),
        body: JSON.stringify({ expectedVersion: 2 }),
      }),
    ]);
    for (const [url, init] of calls) {
      expect(String(url)).not.toContain('Operator+Alerts');
      expect(String(url)).not.toContain(proof);
      expect(JSON.stringify(init?.headers)).not.toContain('Operator+Alerts');
      expect(JSON.stringify(init?.headers)).not.toContain(proof);
      expect(String(init?.body)).not.toContain('must-not-cross-the-client-boundary');
    }
  });

  it('rejects private malformed input locally without reflecting it', async () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    const privateAddress = ' private.owner@example.test';
    const privateProof = 'a3srcv1.private\nproof.secret';

    for (const address of [
      privateAddress,
      'owner@@example.test',
      'owner@-example.test',
      'owner@\u4f8b\u5b50.test',
    ]) {
      try {
        await api.requestRecipientContactVerification('organization', { address }, 'recipient:request');
        throw new Error('invalid address was accepted');
      } catch (error) {
        expect(String(error)).not.toContain(address);
        expect(String(error)).toContain('bounded canonical ASCII mailbox');
      }
    }
    try {
      await api.completeRecipientContactVerification(
        'organization',
        'contact',
        { proof: privateProof },
        'recipient:verify'
      );
      throw new Error('invalid proof was accepted');
    } catch (error) {
      expect(String(error)).not.toContain(privateProof);
      expect(String(error)).toContain('recipient contact proof is invalid');
    }
    expect(() => api.revokeRecipientContact('organization', 'contact', 0, 'recipient:revoke')).toThrow(
      'expected recipient contact version must be a positive safe integer'
    );
    expect(called).toBe(false);
  });

  it('maps transport failure without reflecting proof material', async () => {
    const proof = 'a3srcv1.opaque_payload.opaque_authenticator';
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        throw new Error(`provider rejected ${proof}`);
      },
    });

    try {
      await api.completeRecipientContactVerification(
        'organization',
        'contact',
        { proof },
        'recipient:verify'
      );
      throw new Error('transport failure was accepted');
    } catch (error) {
      expect(String(error)).toContain('Cloud API request failed');
      expect(String(error)).not.toContain(proof);
    }
  });
});
