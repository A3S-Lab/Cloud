import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const PROFILE_ID = '019c0000-0000-7000-8000-000000000004';
const REVISION_ID = '019c0000-0000-7000-8000-000000000005';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000006';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const ACL = 'connector_http { schema = "cloud.connector.http.v1" }\n';

describe('a3s-cloud Connector commands', () => {
  it.each([
    [
      ['connector-profiles', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
        '/connector-profiles?limit=50',
      [profile()],
    ],
    [
      ['connector-profiles', 'get', PROFILE_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
        `/connector-profiles/${PROFILE_ID}`,
      record(),
    ],
    [
      ['connector-revisions', 'list', PROFILE_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
        `/connector-profiles/${PROFILE_ID}/revisions?limit=50`,
      [revision()],
    ],
    [
      ['connector-revisions', 'get', PROFILE_ID, REVISION_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
        `/connector-profiles/${PROFILE_ID}/revisions/${REVISION_ID}`,
      revision(),
    ],
  ] as const)('reads Connector authority %#', async (argv, path, data) => {
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
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it('creates one named Connector profile from bounded A3S ACL', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'connector-profiles',
        'create',
        'Incident webhook',
        '--file=connector.acl',
        '--idempotency-key=cli:connector:create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('connector.acl');
          return new TextEncoder().encode(ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ record: record(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/connector-profiles`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ name: 'Incident webhook', definitionAcl: ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:connector:create',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('revises one Connector profile with the shared optimistic-concurrency option', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'connector-profiles',
        'revise',
        PROFILE_ID,
        '--expected-version=1',
        '--file=connector.acl',
        '--idempotency-key=cli:connector:revise',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new TextEncoder().encode(ACL),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ record: record(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
        `/connector-profiles/${PROFILE_ID}/revisions`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ expectedVersion: 1, definitionAcl: ACL }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:connector:revise' }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects oversized or unversioned Connector ACL mutation before transport', async () => {
    let called = false;
    const output = capture();
    const oversized = await runCli(
      [
        'connector-profiles',
        'create',
        'Webhook',
        '--file=connector.acl',
        '--idempotency-key=cli:connector:oversized',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new Uint8Array(MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES + 1),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );
    expect(oversized).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('Connector definition ACL must contain between');

    const unversioned = await runCli(
      [
        'connector-profiles',
        'revise',
        PROFILE_ID,
        '--file=connector.acl',
        '--idempotency-key=cli:connector:unversioned',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new TextEncoder().encode(ACL),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );
    expect(unversioned).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('--expected-version must be a positive safe integer');
    expect(called).toBe(false);
  });
});

function profile() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    profileId: PROFILE_ID,
    name: 'Incident webhook',
    currentRevisionId: REVISION_ID,
    currentRevisionNumber: 1,
    currentRevisionDigest: DIGEST,
    aggregateVersion: 1,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-15T00:00:00.000Z',
    updatedAt: '2026-08-15T00:00:00.000Z',
  };
}

function revision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    profileId: PROFILE_ID,
    revisionId: REVISION_ID,
    revisionNumber: 1,
    parentRevisionId: null,
    parentDigest: null,
    definitionKind: 'http',
    definitionSchema: 'cloud.connector.http.v1',
    definitionAcl: ACL,
    definitionDigest: DIGEST,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-15T00:00:00.000Z',
  };
}

function record() {
  return { profile: profile(), revision: revision() };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-15T00:00:00.000Z',
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
