import { describe, expect, it } from 'bun:test';
import { CloudApi, CloudApiError, type CloudFetch } from './api';

const CREDENTIAL_ID = '019d0000-0000-7000-8000-000000000001';
const ORGANIZATION_ID = '019d0000-0000-7000-8000-000000000003';
const PROJECT_ID = '019d0000-0000-7000-8000-000000000004';
const ENVIRONMENT_ID = '019d0000-0000-7000-8000-000000000005';
const SECRET = `a3s_mcp_${'a'.repeat(80)}`;

describe('CloudApi MCP credentials', () => {
  it('uses exact tenant paths and no-store transport for the complete lifecycle', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      if (calls.length === 1) {
        return secureResponse([credential()]);
      }
      if (calls.length === 2) {
        return secureResponse(credential());
      }
      if (calls.length === 5) {
        return secureResponse(credential({ replayed: false }));
      }
      return secureResponse(credential({ secret: SECRET, replayed: false }), 201);
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const scope = ['organization / one', 'project / one', 'environment / one'] as const;
    const expiry = { expiresAt: '2027-01-02T03:04:05Z' };

    await api.listMcpCredentials(...scope);
    await api.getMcpCredential(...scope, CREDENTIAL_ID);
    await api.issueMcpCredential(...scope, expiry, 'cli:mcp-issue');
    await api.rotateMcpCredential(...scope, CREDENTIAL_ID, expiry, 'cli:mcp-rotate');
    await api.revokeMcpCredential(...scope, CREDENTIAL_ID, 'cli:mcp-revoke');

    const collection =
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one' +
      '/environments/environment%20%2F%20one/mcp-credentials';
    expect(
      calls.map(([input, init]) => {
        const headers = init?.headers as Record<string, string> | undefined;
        return {
          input,
          method: init?.method,
          idempotencyKey: headers?.['Idempotency-Key'],
          cacheControl: headers?.['Cache-Control'],
          pragma: headers?.Pragma,
          body: init?.body,
        };
      })
    ).toEqual([
      {
        input: collection,
        method: 'GET',
        idempotencyKey: undefined,
        cacheControl: 'no-store',
        pragma: 'no-cache',
        body: undefined,
      },
      {
        input: `${collection}/${CREDENTIAL_ID}`,
        method: 'GET',
        idempotencyKey: undefined,
        cacheControl: 'no-store',
        pragma: 'no-cache',
        body: undefined,
      },
      {
        input: collection,
        method: 'POST',
        idempotencyKey: 'cli:mcp-issue',
        cacheControl: 'no-store',
        pragma: 'no-cache',
        body: JSON.stringify(expiry),
      },
      {
        input: `${collection}/${CREDENTIAL_ID}/rotate`,
        method: 'POST',
        idempotencyKey: 'cli:mcp-rotate',
        cacheControl: 'no-store',
        pragma: 'no-cache',
        body: JSON.stringify(expiry),
      },
      {
        input: `${collection}/${CREDENTIAL_ID}`,
        method: 'DELETE',
        idempotencyKey: 'cli:mcp-revoke',
        cacheControl: 'no-store',
        pragma: 'no-cache',
        body: undefined,
      },
    ]);
  });

  it('rejects unsafe expiry input before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return secureResponse({});
      },
    });

    expect(() =>
      api.issueMcpCredential('organization', 'project', 'environment', { expiresAt: 'tomorrow' }, 'mcp:issue')
    ).toThrow('MCP credential expiry must be an RFC 3339 timestamp');
    expect(() =>
      api.rotateMcpCredential(
        'organization',
        'project',
        'environment',
        CREDENTIAL_ID,
        { expiresAt: '2027-02-30T03:04:05Z' },
        'mcp:rotate'
      )
    ).toThrow('MCP credential expiry must be an RFC 3339 timestamp');
    const expiryWithUnknownField = {
      expiresAt: '2027-01-02T03:04:05Z',
      unexpected: true,
    };
    expect(() =>
      api.issueMcpCredential('organization', 'project', 'environment', expiryWithUnknownField, 'mcp:issue')
    ).toThrow('MCP credential expiry must be an RFC 3339 timestamp');
    expect(called).toBe(false);
  });

  it('fails closed before parsing a credential body without response cache protections', async () => {
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () =>
        new Response(
          JSON.stringify({
            code: 201,
            message: 'Success',
            data: credential({ secret: SECRET, replayed: false }),
            requestId: '019d0000-0000-7000-8000-000000000002',
            timestamp: '2026-07-30T00:00:00.000Z',
          }),
          { status: 201, headers: { 'content-type': 'application/json' } }
        ),
    });

    let failure: unknown;
    try {
      await api.issueMcpCredential(
        'organization',
        'project',
        'environment',
        { expiresAt: '2027-01-02T03:04:05Z' },
        'mcp:issue'
      );
    } catch (error) {
      failure = error;
    }
    expect(failure).toBeInstanceOf(CloudApiError);
    expect(failure).toMatchObject({
      status: 201,
      statusCode: 'INVALID_RESPONSE',
      message: 'Cloud API credential response is missing no-store protections',
    });
    expect(String(failure)).not.toContain(SECRET);
  });

  it('rejects credential material on metadata surfaces and internal material on delivery surfaces', async () => {
    const unsafeResponses = [
      secureResponse([credential({ secret: SECRET })]),
      secureResponse(credential({ unexpectedPublicField: 'must-not-cross-the-api' })),
      secureResponse(
        credential({ secret: SECRET, verifierHash: 'must-not-cross-the-api', replayed: false }),
        201
      ),
    ];
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => unsafeResponses.shift() as Response,
    });

    const failures: unknown[] = [];
    try {
      await api.listMcpCredentials('organization', 'project', 'environment');
    } catch (error) {
      failures.push(error);
    }
    try {
      await api.getMcpCredential('organization', 'project', 'environment', CREDENTIAL_ID);
    } catch (error) {
      failures.push(error);
    }
    try {
      await api.issueMcpCredential(
        'organization',
        'project',
        'environment',
        { expiresAt: '2027-01-02T03:04:05Z' },
        'mcp:issue'
      );
    } catch (error) {
      failures.push(error);
    }

    expect(failures).toHaveLength(3);
    for (const failure of failures) {
      expect(failure).toBeInstanceOf(CloudApiError);
      expect(failure).toMatchObject({
        status: 0,
        statusCode: 'INVALID_RESPONSE',
        message: 'Cloud API returned an invalid MCP credential response',
      });
      expect(String(failure)).not.toContain(SECRET);
      expect(String(failure)).not.toContain('must-not-cross-the-api');
    }
  });
});

function secureResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019d0000-0000-7000-8000-000000000002',
      timestamp: '2026-07-30T00:00:00.000Z',
    }),
    {
      status,
      headers: {
        'cache-control': 'private, no-store',
        'content-type': 'application/json',
        pragma: 'no-cache',
      },
    }
  );
}

function credential(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: CREDENTIAL_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    prefix: `a3s_mcp_${'a'.repeat(16)}`,
    generation: 1,
    aggregateVersion: 1,
    expiresAt: '2027-01-02T03:04:05.000Z',
    createdAt: '2026-07-30T00:00:00.000Z',
    updatedAt: '2026-07-30T00:00:00.000Z',
    revokedAt: null,
    ...extra,
  };
}
