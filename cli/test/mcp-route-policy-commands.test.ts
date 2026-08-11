import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const ROUTE_ID = '019c0000-0000-7000-8000-000000000050';
const ACL = `mcp_route_policy "${ROUTE_ID}" { policy_revision = 1 }`;

describe('a3s-cloud MCP route policy commands', () => {
  it.each([
    [
      ['mcp-routes', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/mcp-route-policies`,
      [policy()],
    ],
    [
      ['mcp-routes', 'get', ROUTE_ID],
      `/organizations/${ORGANIZATION_ID}/mcp-route-policies/${ROUTE_ID}`,
      policy(),
    ],
  ] as const)('queries the authoritative MCP route policy lifecycle %#', async (argv, path, data) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli([...argv, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(data);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      argv: ['mcp-routes', 'create'],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/mcp-route-policies`,
      key: 'cli:mcp-route:create-1',
    },
    {
      argv: ['mcp-routes', 'revise', ROUTE_ID],
      path: `/organizations/${ORGANIZATION_ID}/mcp-route-policies/${ROUTE_ID}/revisions`,
      key: 'cli:mcp-route:revise-1',
    },
  ])('sends one bounded ACL $argv mutation through the shared transport', async ({ argv, path, key }) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli(
      [...argv, '--file=mcp-route.acl', `--idempotency-key=${key}`, '--output=json'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ policy: policy(), replayed: false }, 201);
        },
        readFile: async (path_) => {
          expect(path_).toBe('mcp-route.acl');
          return new TextEncoder().encode(ACL);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': key,
        }),
        body: ACL,
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects an oversized MCP route policy ACL before transport', async () => {
    let called = false;
    const output = capture();
    const exitCode = await runCli(
      ['mcp-routes', 'create', '--file=mcp-route.acl', '--idempotency-key=cli:mcp-route:oversized'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async () => {
          called = true;
          return envelope({});
        },
        readFile: async () => new Uint8Array(524_289),
      }
    );

    expect(exitCode).toBe(ExitCode.Usage);
    expect(called).toBe(false);
    expect(output.stderr()).toContain('MCP route policy ACL must contain between');
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

function policy() {
  return {
    id: ROUTE_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    gatewayScopeId: '019c0000-0000-7000-8000-000000000051',
    domainClaimId: '019c0000-0000-7000-8000-000000000052',
    workloadId: '019c0000-0000-7000-8000-000000000053',
    assetId: '019c0000-0000-7000-8000-000000000054',
    assetReleaseId: '019c0000-0000-7000-8000-000000000055',
    profileDigest: `sha256:${'a'.repeat(64)}`,
    hostname: 'mcp.example.test',
    path: '/mcp',
    tlsRequired: true,
    allowedOrigins: ['https://console.example.test'],
    maxHeaderBytes: 32_768,
    maxRequestBytes: 524_288,
    maxResponseBytes: 4_194_304,
    firstResponseTimeoutSeconds: 30,
    streamIdleTimeoutSeconds: 120,
    streamTotalTimeoutSeconds: 1_800,
    drainTimeoutSeconds: 30,
    telemetryNames: ['weather'],
    telemetryEventsPerMinute: 10_000,
    auditRequired: true,
    grants: [],
    policyRevision: 1,
    policyDigest: `sha256:${'b'.repeat(64)}`,
    acl: ACL,
    expiresAt: '2026-08-07T01:00:00.000Z',
    createdAt: '2026-08-07T00:00:00.000Z',
    updatedAt: '2026-08-07T00:00:00.000Z',
  };
}
