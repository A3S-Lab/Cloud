import { describe, expect, it } from 'bun:test';
import type { AuditExport, AuditRecordPage, AuditRetentionStatus, CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const ACTOR_ID = '019c0000-0000-7000-8000-000000000002';
const AGGREGATE_ID = '019c0000-0000-7000-8000-000000000003';
const REQUEST_ID = '019c0000-0000-7000-8000-000000000004';
const AUDIT_ID = '019c0000-0000-7000-8000-000000000005';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000006';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000007';
const ATTRIBUTION_PROFILE_ID = '019c0000-0000-7000-8000-000000000008';

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
      projectId: PROJECT_ID,
      environmentId: ENVIRONMENT_ID,
      attributionProfileId: ATTRIBUTION_PROFILE_ID,
      attributionStatus: 'profile_bound',
    },
  ],
  nextCursor: `v1:1786582923000000:${AUDIT_ID}`,
};

const SIGNED_EXPORT: AuditExport = {
  envelope: {
    payloadType: 'application/vnd.a3s.cloud.audit-export.v1+json',
    payload: 'e30=',
    signatures: [{ keyId: 'a'.repeat(64), signature: 'c2lnbmF0dXJl' }],
  },
  signingKey: {
    algorithm: 'ed25519',
    keyId: 'a'.repeat(64),
    publicKey: 'cHVibGljLWtleQ==',
  },
};

const RETENTION_STATUS: AuditRetentionStatus = {
  organizationId: ORGANIZATION_ID,
  retentionMs: 7_776_000_000,
  policyDigest: `sha256:${'a'.repeat(64)}`,
  appliedPolicyDigest: `sha256:${'a'.repeat(64)}`,
  currentPolicyApplied: true,
  recordsAvailableFrom: '2026-05-25T12:00:00Z',
  recordsDeletedBefore: '2026-05-25T12:00:00Z',
  totalDeletedRecords: 42,
  lastSweptAt: '2026-08-23T12:00:00Z',
  lastCompletedAt: '2026-08-23T12:00:00Z',
  nextScanAt: '2026-08-23T12:01:00Z',
  version: 7,
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

describe('audit-records commands', () => {
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
        `--project=${PROJECT_ID}`,
        `--environment=${ENVIRONMENT_ID}`,
        `--attribution-profile=${ATTRIBUTION_PROFILE_ID}`,
        '--attribution-status=profile_bound',
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
    expect(String(calls[0]?.[0])).toContain(`projectId=${PROJECT_ID}`);
    expect(String(calls[0]?.[0])).toContain('attributionStatus=profile_bound');
    expect(String(calls[0]?.[0])).toContain('limit=25');
    expect(output.stdout()).toContain('OCCURRED AT');
    expect(output.stdout()).toContain('identity.membership.created');
    expect(output.stdout()).toContain('Next cursor: v1:');
    expect(output.stdout()).not.toContain('details');
    expect(output.stderr()).toBe('');
  });

  it('exports the complete signed envelope for an explicit bounded window', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      ['audit-records', 'export', '--from=2026-08-01T00:00:00Z', '--to=2026-08-13T00:00:00Z', '--limit=25'],
      {
        ...output.runtime,
        environment: {
          A3S_CLOUD_TOKEN: 'token',
          A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope(SIGNED_EXPORT);
        },
      }
    );

    expect(exitCode).toBe(0);
    expect(String(calls[0]?.[0])).toContain(`/organizations/${ORGANIZATION_ID}/audit-records/export?`);
    expect(String(calls[0]?.[0])).toContain('from=2026-08-01T00%3A00%3A00Z');
    expect(output.stdout()).toContain('application/vnd.a3s.cloud.audit-export.v1+json');
    expect(output.stdout()).toContain('"payload": "e30="');
    expect(output.stderr()).toBe('');
  });

  it('shows the enforced retention policy and monotonic organization watermarks', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(['audit-records', 'retention'], {
      ...output.runtime,
      environment: {
        A3S_CLOUD_TOKEN: 'token',
        A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
      },
      fetch: async (...args) => {
        calls.push(args);
        return envelope(RETENTION_STATUS);
      },
    });

    expect(exitCode).toBe(0);
    expect(String(calls[0]?.[0])).toContain(
      `/api/v1/organizations/${ORGANIZATION_ID}/audit-records/retention`
    );
    expect(output.stdout()).toContain('"retentionMs": 7776000000');
    expect(output.stdout()).toContain('"recordsAvailableFrom": "2026-05-25T12:00:00Z"');
    expect(output.stdout()).toContain('"totalDeletedRecords": 42');
    expect(output.stderr()).toBe('');
  });

  it.each([
    [['audit-records', 'list', '--limit=0'], 'audit record limit must be between 1 and 200'],
    [['audit-records', 'list', '--cursor='], 'option --cursor requires a value'],
    [['audit-records', 'list', '--action=Invalid'], 'audit action must use bounded lowercase'],
    [['audit-records', 'list', '--attribution-status=invalid'], 'audit attribution status is invalid'],
    [
      ['audit-records', 'list', '--from=2026-08-14T00:00:00Z', '--to=2026-08-13T00:00:00Z'],
      'audit from timestamp must not exceed to timestamp',
    ],
    [['audit-records', 'export', '--from=2026-08-01T00:00:00Z'], '--to is required'],
    [
      ['audit-records', 'export', '--from=2026-07-01T00:00:00Z', '--to=2026-08-02T00:00:00Z'],
      'audit export window must not exceed 31 days',
    ],
    [
      ['audit-records', 'retention', '--limit=1'],
      'audit-records retention does not accept record query options',
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
