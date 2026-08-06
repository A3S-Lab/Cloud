import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const CREDENTIAL_ID = '019c0000-0000-7000-8000-000000000040';
const BEARER = `a3s_mcp_0123456789abcdef${'a'.repeat(64)}`;

describe('a3s-cloud hosted MCP credential commands', () => {
  it('creates and prints the bounded one-time bearer delivery', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(
        {
          credential: credential(),
          bearerCredential: BEARER,
          deliveryExpiresAt: '2026-08-06T00:10:00.000Z',
          replayed: false,
        },
        201
      );
    };
    const output = capture();

    const exitCode = await runCli(
      [
        'mcp-credentials',
        'create',
        '--expires-at=2026-09-06T00:00:00Z',
        '--idempotency-key=cli:mcp:create:1',
        '--output=json',
      ],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/mcp-credentials`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:mcp:create:1' }),
        body: JSON.stringify({ expiresAt: '2026-09-06T00:00:00.000Z' }),
      })
    );
    expect(output.stdout()).toContain(BEARER);
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      action: 'rotate',
      extra: ['--expires-at=2026-10-06T00:00:00Z'],
      body: { expiresAt: '2026-10-06T00:00:00.000Z', expectedAggregateVersion: 1 },
      response: {
        credential: { ...credential(), generation: 2, aggregateVersion: 2 },
        bearerCredential: BEARER,
        deliveryExpiresAt: '2026-08-06T00:10:00.000Z',
        replayed: false,
      },
    },
    {
      action: 'revoke',
      extra: [],
      body: { expectedAggregateVersion: 1 },
      response: {
        credential: { ...credential(), state: 'revoked', aggregateVersion: 2 },
        replayed: false,
      },
    },
  ])('sends an explicit optimistic $action mutation', async ({ action, extra, body, response }) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(response);
    };
    const output = capture();

    const exitCode = await runCli(
      [
        'mcp-credentials',
        action,
        CREDENTIAL_ID,
        '--expected-version=1',
        ...extra,
        `--idempotency-key=cli:mcp:${action}:1`,
      ],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/mcp-credentials/${CREDENTIAL_ID}/${action}`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': `cli:mcp:${action}:1` }),
        body: JSON.stringify(body),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects missing expiry and optimistic versions before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    for (const argv of [
      ['mcp-credentials', 'create', '--idempotency-key=missing-expiry'],
      [
        'mcp-credentials',
        'rotate',
        CREDENTIAL_ID,
        '--expires-at=2026-10-06T00:00:00Z',
        '--idempotency-key=missing-version',
      ],
      ['mcp-credentials', 'revoke', CREDENTIAL_ID, '--idempotency-key=missing-version'],
    ]) {
      const output = capture();
      expect(
        await runCli(argv, {
          ...output.runtime,
          environment: completeEnvironment(),
          fetch: fetcher,
        })
      ).toBe(2);
    }
    expect(called).toBe(false);
  });
});

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-06T00:00:00.000Z',
    }),
    { status }
  );
}

function capture() {
  let stdout = '';
  let stderr = '';
  return {
    runtime: {
      writeStdout: (value: string) => {
        stdout += value;
      },
      writeStderr: (value: string) => {
        stderr += value;
      },
    },
    stdout: () => stdout,
    stderr: () => stderr,
  };
}

function completeEnvironment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
    A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
  };
}

function credential() {
  return {
    id: CREDENTIAL_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    prefix: 'a3s_mcp_0123456789abcdef',
    state: 'active',
    generation: 1,
    aggregateVersion: 1,
    expiresAt: '2026-09-06T00:00:00.000Z',
    createdAt: '2026-08-06T00:00:00.000Z',
    updatedAt: '2026-08-06T00:00:00.000Z',
    revokedAt: null,
  };
}
