import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const ENDPOINT_ID = '019c0000-0000-7000-8000-000000000010';
const AUTOMATION_ID = '019c0000-0000-7000-8000-000000000011';
const REVISION_ID = '019c0000-0000-7000-8000-000000000012';
const SECRET_ID = '019c0000-0000-7000-8000-000000000013';

describe('a3s-cloud Automation webhook and definition commands', () => {
  it('creates one webhook endpoint without Idempotency-Key', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'automation-webhook-endpoints',
        'create',
        ENDPOINT_ID,
        'release-hook',
        SECRET_ID,
        '1',
        '65536',
        AUTOMATION_ID,
        REVISION_ID,
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope(endpoint(), 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/automation-webhook-endpoints`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          endpointId: ENDPOINT_ID,
          endpointKey: 'release-hook',
          signingSecret: { secretId: SECRET_ID, version: 1 },
          maxBodyBytes: 65536,
          automationId: AUTOMATION_ID,
          revisionId: REVISION_ID,
        }),
        headers: expect.not.objectContaining({
          'Idempotency-Key': expect.anything(),
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it.each([
    ['disable', 1],
    ['enable', 2],
    ['revoke', 3],
  ] as const)('applies generation-fenced %s without Idempotency-Key', async (action, generation) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'automation-webhook-endpoints',
        action,
        ENDPOINT_ID,
        `--expected-version=${generation}`,
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ ...endpoint(), generation, state: action === 'revoke' ? 'revoked' : action === 'disable' ? 'disabled' : 'active' });
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/automation-webhook-endpoints/${ENDPOINT_ID}/${action}`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ expectedGeneration: generation }),
        headers: expect.not.objectContaining({
          'Idempotency-Key': expect.anything(),
        }),
      })
    );
  });

  it.each([
    [
      ['automation-webhook-endpoints', 'get', ENDPOINT_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
        `/automation-webhook-endpoints/${ENDPOINT_ID}`,
      endpoint(),
    ],
    [
      ['automation-definitions', 'list'],
      `/organizations/${ORGANIZATION_ID}/automation-definitions?limit=50`,
      [definition()],
    ],
    [
      ['automation-definitions', 'get', AUTOMATION_ID],
      `/organizations/${ORGANIZATION_ID}/automation-definitions/${AUTOMATION_ID}`,
      definition(),
    ],
    [
      ['automation-revisions', 'get', AUTOMATION_ID, REVISION_ID],
      `/organizations/${ORGANIZATION_ID}/automation-definitions/${AUTOMATION_ID}/revisions/${REVISION_ID}`,
      revision(),
    ],
  ] as const)('reads Automation authority %#', async (argv, path, data) => {
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

  it('rejects create when --idempotency-key is supplied', async () => {
    const output = capture();
    const exitCode = await runCli(
      [
        'automation-webhook-endpoints',
        'create',
        ENDPOINT_ID,
        'release-hook',
        SECRET_ID,
        '1',
        '65536',
        AUTOMATION_ID,
        REVISION_ID,
        '--idempotency-key=cli:automation:create',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async () => {
          throw new Error('fetch must not run');
        },
      }
    );
    expect(exitCode).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('idempotency');
  });
});

function endpoint() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    endpointId: ENDPOINT_ID,
    endpointKey: 'release-hook',
    state: 'active',
    generation: 1,
    signingSecret: { secretId: SECRET_ID, version: 1 },
    maxBodyBytes: 65536,
    automationId: AUTOMATION_ID,
    revisionId: REVISION_ID,
    createdAt: '2026-08-15T00:00:00.000Z',
    updatedAt: '2026-08-15T00:00:00.000Z',
  };
}

function definition() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    automationId: AUTOMATION_ID,
    name: 'release-hook',
    triggerKind: 'webhook',
    revisionId: REVISION_ID,
    revisionNumber: 1,
    revisionDigest: `sha256:${'a'.repeat(64)}`,
    updatedAt: '2026-08-15T00:00:00.000Z',
  };
}

function revision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    automationId: AUTOMATION_ID,
    revisionId: REVISION_ID,
    revisionNumber: 1,
    revisionDigest: `sha256:${'a'.repeat(64)}`,
    parentRevisionId: null,
    definitionAcl: 'automation { }\n',
    createdAt: '2026-08-15T00:00:00.000Z',
  };
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
