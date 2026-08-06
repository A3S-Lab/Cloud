import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';

const credential = {
  id: '0198b55e-43f0-7b11-a010-123456789abc',
  organizationId: '0198b55e-43f0-7b11-a010-123456789abd',
  projectId: '0198b55e-43f0-7b11-a010-123456789abe',
  environmentId: '0198b55e-43f0-7b11-a010-123456789abf',
  prefix: 'a3s_mcp_0123456789abcdef',
  state: 'active' as const,
  generation: 1,
  aggregateVersion: 1,
  expiresAt: '2026-09-01T00:00:00Z',
  createdAt: '2026-08-01T00:00:00Z',
  updatedAt: '2026-08-01T00:00:00Z',
  revokedAt: null,
};

function envelope(data: unknown): Response {
  return Response.json({
    code: 200,
    message: 'Success',
    data,
    requestId: '0198b55e-43f0-7b11-a010-123456789ac0',
    timestamp: '2026-08-01T00:00:00Z',
  });
}

describe('hosted MCP credential client', () => {
  it('uses one REST lifecycle with explicit idempotency and optimistic versions', async () => {
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    const fetcher: CloudFetch = async (input, init) => {
      calls.push({ input: String(input), init });
      if (String(input).endsWith('/mcp-credentials') && init?.method === 'GET') {
        return envelope([credential]);
      }
      if (String(input).endsWith(`/${credential.id}`)) {
        return envelope(credential);
      }
      if (String(input).endsWith('/rotate')) {
        return envelope({
          credential: { ...credential, generation: 2, aggregateVersion: 2 },
          bearerCredential: `a3s_mcp_0123456789abcdef${'a'.repeat(64)}`,
          deliveryExpiresAt: '2026-08-01T00:10:00Z',
          replayed: false,
        });
      }
      if (String(input).endsWith('/revoke')) {
        return envelope({
          credential: { ...credential, state: 'revoked', aggregateVersion: 3 },
          replayed: false,
        });
      }
      return envelope({
        credential,
        bearerCredential: `a3s_mcp_0123456789abcdef${'b'.repeat(64)}`,
        deliveryExpiresAt: '2026-08-01T00:10:00Z',
        replayed: false,
      });
    };
    const api = new CloudApi('token', 'https://cloud.test/api/v1', { fetch: fetcher });

    await api.listMcpCredentials(credential.organizationId, credential.projectId, credential.environmentId);
    await api.getMcpCredential(credential.organizationId, credential.id);
    await api.createMcpCredential(
      credential.organizationId,
      credential.projectId,
      credential.environmentId,
      { expiresAt: '2026-09-01T00:00:00Z' },
      'cli:mcp:create:1'
    );
    await api.rotateMcpCredential(
      credential.organizationId,
      credential.id,
      { expiresAt: '2026-10-01T00:00:00Z', expectedAggregateVersion: 1 },
      'cli:mcp:rotate:1'
    );
    await api.revokeMcpCredential(
      credential.organizationId,
      credential.id,
      { expectedAggregateVersion: 2 },
      'cli:mcp:revoke:1'
    );

    expect(calls.map((call) => call.init?.method)).toEqual(['GET', 'GET', 'POST', 'POST', 'POST']);
    expect(calls[2]?.init?.headers).toMatchObject({ 'Idempotency-Key': 'cli:mcp:create:1' });
    expect(calls[3]?.init?.headers).toMatchObject({ 'Idempotency-Key': 'cli:mcp:rotate:1' });
    expect(calls[4]?.init?.headers).toMatchObject({ 'Idempotency-Key': 'cli:mcp:revoke:1' });
    expect(JSON.parse(String(calls[3]?.init?.body))).toEqual({
      expiresAt: '2026-10-01T00:00:00Z',
      expectedAggregateVersion: 1,
    });
    expect(JSON.parse(String(calls[4]?.init?.body))).toEqual({ expectedAggregateVersion: 2 });
  });

  it('rejects invalid expiry and versions before transport', async () => {
    let calls = 0;
    const api = new CloudApi('token', 'https://cloud.test/api/v1', {
      fetch: async () => {
        calls += 1;
        return envelope(credential);
      },
    });

    expect(() =>
      api.createMcpCredential(
        credential.organizationId,
        credential.projectId,
        credential.environmentId,
        { expiresAt: 'tomorrow' },
        'create'
      )
    ).toThrow('MCP credential expiry must be an RFC 3339 timestamp');
    expect(() =>
      api.rotateMcpCredential(
        credential.organizationId,
        credential.id,
        { expiresAt: '2026-10-01T00:00:00Z', expectedAggregateVersion: 0 },
        'rotate'
      )
    ).toThrow('expected MCP credential version must be a positive safe integer');
    expect(calls).toBe(0);
  });
});
