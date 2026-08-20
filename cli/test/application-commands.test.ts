import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_APPLICATION_RELEASE_ACL_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const APPLICATION_ID = '019c0000-0000-7000-8000-000000000003';
const RELEASE_ID = '019c0000-0000-7000-8000-000000000004';
const WORKFLOW_DEFINITION_ID = '019c0000-0000-7000-8000-000000000005';
const WORKFLOW_REVISION_ID = '019c0000-0000-7000-8000-000000000006';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000007';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const ACL = 'application_release { schema = "cloud.application.release.v1" }\n';

describe('a3s-cloud Application commands', () => {
  it.each([
    [
      ['applications', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/applications?limit=50`,
      [application()],
    ],
    [
      ['applications', 'get', APPLICATION_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/applications/${APPLICATION_ID}`,
      application(),
    ],
    [
      ['application-releases', 'list', APPLICATION_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/applications/${APPLICATION_ID}` +
        '/releases?limit=50',
      [release()],
    ],
    [
      ['application-releases', 'get', APPLICATION_ID, RELEASE_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/applications/${APPLICATION_ID}` +
        `/releases/${RELEASE_ID}`,
      release(),
    ],
  ] as const)('reads Application authority %#', async (argv, path, data) => {
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

  it('creates one named Application from bounded release ACL', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'applications',
        'create',
        'Support assistant',
        '--file=application.acl',
        '--idempotency-key=cli:application:create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('application.acl');
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
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` + `/projects/${PROJECT_ID}/applications`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          name: 'Support assistant',
          description: '',
          releaseAcl: ACL,
        }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:application:create',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('publishes one immutable release with optimistic concurrency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'applications',
        'publish',
        APPLICATION_ID,
        '--expected-version=1',
        '--file=application.acl',
        '--idempotency-key=cli:application:publish',
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
        `/projects/${PROJECT_ID}/applications/${APPLICATION_ID}/releases`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ expectedVersion: 1, releaseAcl: ACL }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:application:publish' }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects oversized or unversioned Application ACL mutation before transport', async () => {
    let called = false;
    const output = capture();
    const oversized = await runCli(
      [
        'applications',
        'create',
        'Assistant',
        '--file=application.acl',
        '--idempotency-key=cli:application:oversized',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new Uint8Array(MAX_APPLICATION_RELEASE_ACL_BYTES + 1),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );
    expect(oversized).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('Application release ACL must contain between');

    const unversioned = await runCli(
      [
        'applications',
        'publish',
        APPLICATION_ID,
        '--file=application.acl',
        '--idempotency-key=cli:application:unversioned',
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

function application() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    applicationId: APPLICATION_ID,
    name: 'Support assistant',
    description: '',
    experience: 'chatbot',
    currentReleaseId: RELEASE_ID,
    currentReleaseNumber: 1,
    currentReleaseDigest: DIGEST,
    aggregateVersion: 1,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-20T00:00:00.000Z',
    updatedAt: '2026-08-20T00:00:00.000Z',
  };
}

function release() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    applicationId: APPLICATION_ID,
    releaseId: RELEASE_ID,
    releaseNumber: 1,
    parentReleaseId: null,
    parentDigest: null,
    experience: 'chatbot',
    audience: 'project_members',
    interactionMode: 'conversation',
    responseModes: ['blocking'],
    contractSchema: 'cloud.application.release.v1',
    contractAcl: ACL,
    contractDigest: DIGEST,
    workflowDefinitionId: WORKFLOW_DEFINITION_ID,
    workflowRevisionId: WORKFLOW_REVISION_ID,
    workflowContractDigest: DIGEST,
    workflowPayloadSetDigest: DIGEST,
    workflowSemanticContractSetDigest: DIGEST,
    inputSchemaDigest: DIGEST,
    outputSchemaDigest: DIGEST,
    presentationDigest: DIGEST,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-20T00:00:00.000Z',
  };
}

function record() {
  return { application: application(), release: release() };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-20T00:00:00.000Z',
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
  };
}
