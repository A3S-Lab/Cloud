import { describe, expect, it } from 'bun:test';
import type { AuditRecordPage, CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const ACTOR_ID = '019c0000-0000-7000-8000-000000000002';
const AGGREGATE_ID = '019c0000-0000-7000-8000-000000000003';
const REQUEST_ID = '019c0000-0000-7000-8000-000000000004';
const AUDIT_ID = '019c0000-0000-7000-8000-000000000005';

const PAGE: AuditRecordPage = {
  records: [
    {
      id: AUDIT_ID,
      organizationId: ORGANIZATION_ID,
      actorPrincipalId: ACTOR_ID,
      action: 'identity.membership.created',
      aggregateId: AGGREGATE_ID,
      occurredAt: '2026-08-13T01:02:03Z',
      requestId: REQUEST_ID,
    },
  ],
  nextCursor: `v1:1786582923000000:${AUDIT_ID}`,
};

function envelope(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: REQUEST_ID,
      timestamp: '2026-08-13T01:02:04Z',
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

describe('audit-records list command', () => {
  it('calls the shared bounded client query and renders redacted history', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'audit-records',
        'list',
        `--actor-principal=${ACTOR_ID}`,
        '--action=identity.membership.created',
        `--aggregate=${AGGREGATE_ID}`,
        `--request-id=${REQUEST_ID}`,
        '--from=2026-08-13T00:00:00Z',
        '--to=2026-08-14T00:00:00Z',
        `--cursor=v1:1786582923000000:${AUDIT_ID}`,
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
    expect(String(calls[0]?.[0])).toContain(`/organizations/${ORGANIZATION_ID}/audit-records?`);
    expect(String(calls[0]?.[0])).toContain('action=identity.membership.created');
    expect(String(calls[0]?.[0])).toContain('limit=25');
    expect(output.stdout()).toContain('OCCURRED AT');
    expect(output.stdout()).toContain('identity.membership.created');
    expect(output.stdout()).toContain('Next cursor: v1:');
    expect(output.stdout()).not.toContain('details');
    expect(output.stderr()).toBe('');
  });

  it.each([
    [['audit-records', 'list', '--limit=0'], 'audit record limit must be between 1 and 200'],
    [['audit-records', 'list', '--cursor='], 'option --cursor requires a value'],
    [['audit-records', 'list', '--action=Invalid'], 'audit action must use bounded lowercase'],
    [
      ['audit-records', 'list', '--from=2026-08-14T00:00:00Z', '--to=2026-08-13T00:00:00Z'],
      'audit from timestamp must not exceed to timestamp',
    ],
    [['organizations', 'list', `--request-id=${REQUEST_ID}`], '--actor-principal, --action, --aggregate'],
  ])('rejects invalid or misplaced options before transport %#', async (argv, message) => {
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
