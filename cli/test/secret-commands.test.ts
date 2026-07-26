import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const SECRET_ID = '019c0000-0000-7000-8000-000000000016';

describe('a3s-cloud Secret commands', () => {
  it.each([
    [
      ['secrets', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/secrets`,
      [],
    ],
    [
      ['secrets', 'get', SECRET_ID],
      `/organizations/${ORGANIZATION_ID}/secrets/${SECRET_ID}`,
      { ...secretResource(), versions: [secretVersion(1)] },
    ],
  ] as const)('queries a Secret resource without exposing material %#', async (command, path, response) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(response);
    };
    const output = capture();

    const exitCode = await runCli([...command, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stdout()).not.toContain('value');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      command: ['secrets', 'create', 'Database URL', '--value-stdin'],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/secrets`,
      value: 'postgres://cloud:initial@database',
      body: (value: string) => ({ name: 'Database URL', value }),
      response: { ...secretResource(), version: secretVersion(1), replayed: false },
    },
    {
      command: ['secrets', 'add-version', SECRET_ID, '--value-stdin'],
      path: `/organizations/${ORGANIZATION_ID}/secrets/${SECRET_ID}/versions`,
      value: 'postgres://cloud:rotated@database',
      body: (value: string) => ({ value }),
      response: {
        ...secretResource(),
        currentVersion: 2,
        aggregateVersion: 2,
        version: secretVersion(2),
        replayed: false,
      },
    },
  ] as const)('reads one idempotent mutation from bounded standard input %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const inputLimits: number[] = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ ...testCase.response, value: testCase.value }, 201);
    };
    const output = capture();

    const exitCode = await runCli([...testCase.command, '--idempotency-key=cli:secret-1', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
      readStdin: async (limitBytes) => {
        inputLimits.push(limitBytes);
        return new TextEncoder().encode(testCase.value);
      },
    });

    expect(exitCode).toBe(0);
    expect(inputLimits).toEqual([1_048_577]);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:secret-1' }),
        body: JSON.stringify(testCase.body(testCase.value)),
      })
    );
    expect(output.stdout()).not.toContain(testCase.value);
    expect(output.stderr()).not.toContain(testCase.value);
  });

  it('sanitizes a rejected mutation even if an upstream error echoes the material', async () => {
    const value = 'postgres://cloud:must-not-leak@database';
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 422,
          statusCode: 'UNPROCESSABLE_ENTITY',
          message: `invalid Secret ${value}`,
          details: { value },
          requestId: '019c0000-0000-7000-8000-000000000017',
          timestamp: '2026-07-27T00:00:00.000Z',
        }),
        { status: 422 }
      );
    const output = capture();

    const exitCode = await runCli(
      [
        'secrets',
        'create',
        'Database URL',
        '--value-stdin',
        '--idempotency-key=cli:secret-rejected',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readStdin: async () => new TextEncoder().encode(value),
      }
    );

    expect(exitCode).toBe(ExitCode.Api);
    expect(output.stderr()).toContain('Secret mutation failed');
    expect(output.stderr()).not.toContain(value);
    expect(output.stdout()).toBe('');
  });

  it('revokes one version without reading material', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({
        ...secretResource(),
        version: { ...secretVersion(1), state: 'revoked' },
        replayed: false,
      });
    };
    const output = capture();

    const exitCode = await runCli(
      ['secrets', 'revoke-version', SECRET_ID, '1', '--idempotency-key=cli:secret-revoke'],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/secrets/${SECRET_ID}/versions/1/revoke`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:secret-revoke' }),
        body: undefined,
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects unsafe Secret input before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const cases: Array<{
      argv: string[];
      input?: Uint8Array;
      failure?: Error;
      message: string;
      hidden?: string;
    }> = [
      {
        argv: ['secrets', 'create', 'Database URL', '--idempotency-key=cli:secret-missing'],
        message: '--value-stdin is required',
      },
      {
        argv: ['secrets', 'create', 'Database URL', '--value-stdin', '--idempotency-key=cli:secret-empty'],
        input: new Uint8Array(),
        message: 'between 1 byte and 1 MiB',
      },
      {
        argv: ['secrets', 'add-version', SECRET_ID, '--value-stdin', '--idempotency-key=cli:secret-utf8'],
        input: Uint8Array.from([0xff]),
        message: 'valid UTF-8',
      },
      {
        argv: ['secrets', 'add-version', SECRET_ID, '--value-stdin', '--idempotency-key=cli:secret-large'],
        input: new Uint8Array(1_048_577),
        message: 'between 1 byte and 1 MiB',
      },
      {
        argv: ['secrets', 'create', 'Database URL', '--value-stdin', '--idempotency-key=cli:secret-read'],
        failure: new Error('reader failed while handling do-not-leak'),
        message: 'unable to read Secret value from standard input',
        hidden: 'do-not-leak',
      },
      {
        argv: ['organizations', 'list', '--value-stdin'],
        message: '--value-stdin is valid only for Secret value mutations',
      },
      {
        argv: ['secrets', 'revoke-version', SECRET_ID, '0', '--idempotency-key=cli:secret-version'],
        message: 'Secret version must be a positive safe integer',
      },
    ];

    for (const testCase of cases) {
      const output = capture();
      const exitCode = await runCli(testCase.argv, {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readStdin: async () => {
          if (testCase.failure) {
            throw testCase.failure;
          }
          return testCase.input ?? new Uint8Array();
        },
      });
      expect(exitCode).toBe(2);
      expect(output.stderr()).toContain(testCase.message);
      if (testCase.hidden) {
        expect(output.stderr()).not.toContain(testCase.hidden);
      }
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
      timestamp: '2026-07-27T00:00:00.000Z',
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

function secretResource(): Record<string, unknown> {
  return {
    id: SECRET_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    name: 'Database URL',
    state: 'active',
    currentVersion: 1,
    aggregateVersion: 1,
    createdAt: '2026-07-27T00:00:00.000Z',
    updatedAt: '2026-07-27T00:00:00.000Z',
    revokedAt: null,
  };
}

function secretVersion(version: number): Record<string, unknown> {
  return {
    version,
    state: 'active',
    aggregateVersion: 1,
    createdAt: '2026-07-27T00:00:00.000Z',
    revokedAt: null,
  };
}
