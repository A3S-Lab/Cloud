import { describe, expect, it } from 'bun:test';
import type { CloudFetch, GatewayRoutePolicyTimelinePage } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const ROUTE_ID = '019c0000-0000-7000-8000-000000000002';
const EVENT_ID = '019c0000-0000-7000-8000-000000000003';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000004';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000005';
const CORRELATION_ID = '019c0000-0000-7000-8000-000000000006';

const PAGE: GatewayRoutePolicyTimelinePage = {
  entries: [
    {
      eventId: EVENT_ID,
      eventKey: 'edge.mcp-route-policy.revised',
      schemaVersion: 1,
      organizationId: ORGANIZATION_ID,
      projectId: PROJECT_ID,
      environmentId: ENVIRONMENT_ID,
      routeId: ROUTE_ID,
      policyRevision: 2,
      policyDigest: `sha256:${'a'.repeat(64)}`,
      occurredAt: '2026-08-23T01:02:03Z',
      correlationId: CORRELATION_ID,
      auditCorrelation: 'missing',
      auditRecordId: null,
      actorPrincipalId: null,
    },
  ],
  nextCursor: `v1:1787446923000000:${EVENT_ID}`,
};

function envelope(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: CORRELATION_ID,
      timestamp: '2026-08-23T01:02:04Z',
    }),
    { status: 200 }
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

describe('security-investigations timeline command', () => {
  it('calls the shared bounded query and renders only redacted typed evidence', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'security-investigations',
        'timeline',
        ROUTE_ID,
        `--cursor=v1:1787446923000000:${EVENT_ID}`,
        '--limit=25',
      ],
      {
        ...output.runtime,
        environment: {
          A3S_CLOUD_TOKEN: 'token',
          A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope(PAGE);
        },
      }
    );

    expect(exitCode).toBe(0);
    expect(String(calls[0]?.[0])).toContain(
      `/organizations/${ORGANIZATION_ID}/security-investigations/gateway-routes/${ROUTE_ID}/timeline?`
    );
    expect(String(calls[0]?.[0])).toContain('limit=25');
    expect(output.stdout()).toContain('POLICY REV');
    expect(output.stdout()).toContain('edge.mcp-route-policy.revised');
    expect(output.stdout()).toContain('missing');
    expect(output.stdout()).toContain('Next cursor: v1:');
    expect(output.stdout()).not.toContain('details');
    expect(output.stderr()).toBe('');
  });

  it.each([
    [['security-investigations', 'timeline', 'not-a-uuid'], 'Gateway Route policy route ID must be a UUID'],
    [
      ['security-investigations', 'timeline', ROUTE_ID, '--limit=0'],
      'security timeline limit must be between 1 and 100',
    ],
    [['security-investigations', 'timeline', ROUTE_ID, '--cursor='], 'option --cursor requires a value'],
  ])('rejects invalid scope or pagination before transport %#', async (argv, message) => {
    let called = false;
    const output = capture();
    const exitCode = await runCli(argv, {
      ...output.runtime,
      environment: {
        A3S_CLOUD_TOKEN: 'token',
        A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
      },
      fetch: async () => {
        called = true;
        return envelope(PAGE);
      },
    });
    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain(message);
  });
});
