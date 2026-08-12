import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_EXECUTION_TEMPLATE_ACL_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const TEMPLATE_ID = '019c0000-0000-7000-8000-000000000003';
const REVISION_ID = '019c0000-0000-7000-8000-000000000004';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000005';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const ACL = 'execution_template "echo" { schema = "cloud.execution-template.v1" }\n';

describe('a3s-cloud ExecutionTemplate commands', () => {
  it.each([
    [
      ['execution-templates', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/execution-templates?limit=100`,
      [revision()],
    ],
    [
      ['execution-templates', 'get', TEMPLATE_ID, REVISION_ID],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/execution-templates/${TEMPLATE_ID}/revisions/${REVISION_ID}`,
      revision(),
    ],
  ] as const)('reads immutable ExecutionTemplate authority %#', async (argv, path, data) => {
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

  it('publishes one bounded ACL-native ExecutionTemplate', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'execution-templates',
        'create',
        '--file=execution-template.acl',
        '--idempotency-key=cli:execution-template:create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('execution-template.acl');
          return new TextEncoder().encode(ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ executionTemplate: revision(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/execution-templates`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ definitionAcl: ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:execution-template:create',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects oversized ACL before transport', async () => {
    let called = false;
    const output = capture();
    const exitCode = await runCli(
      [
        'execution-templates',
        'create',
        '--file=execution-template.acl',
        '--idempotency-key=cli:execution-template:oversized',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new Uint8Array(MAX_EXECUTION_TEMPLATE_ACL_BYTES + 1),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Usage);
    expect(called).toBe(false);
    expect(output.stderr()).toContain('ExecutionTemplate ACL must contain between');
  });
});

function revision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    templateId: TEMPLATE_ID,
    revisionId: REVISION_ID,
    definitionAcl: ACL,
    definitionDigest: DIGEST,
    capability: 'execution.run' as const,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-13T00:00:00.000Z',
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-13T00:00:00.000Z',
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
