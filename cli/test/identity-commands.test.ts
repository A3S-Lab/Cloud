import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const API_TOKEN_ID = '019c0000-0000-7000-8000-000000000018';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000020';
const MEMBERSHIP_ID = '019c0000-0000-7000-8000-000000000021';
const RESOURCE_GRANT_ID = '019c0000-0000-7000-8000-000000000022';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000023';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000024';
const NODE_ID = '019c0000-0000-7000-8000-000000000025';
const API_TOKEN = `a3s_${'a'.repeat(64)}`;

describe('a3s-cloud identity commands', () => {
  it.each([
    [['api-tokens', 'list'], `/organizations/${ORGANIZATION_ID}/api-tokens`, [apiTokenResource()]],
    [
      ['api-tokens', 'get', API_TOKEN_ID],
      `/organizations/${ORGANIZATION_ID}/api-tokens/${API_TOKEN_ID}`,
      apiTokenResource(),
    ],
  ] as const)('queries API token metadata without exposing credentials %#', async (command, path, response) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(addCredential(response));
    };
    const output = capture();

    const exitCode = await runCli([...command, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stdout()).not.toContain(API_TOKEN);
    expect(output.stdout()).not.toContain('token');
    expect(output.stderr()).toBe('');
  });

  it('creates an API token from bounded standard input and clears the input buffer', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const inputLimits: number[] = [];
    const input = new TextEncoder().encode(API_TOKEN);
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ ...apiTokenResource(), token: API_TOKEN, replayed: false }, 201);
    };
    const output = capture();

    const exitCode = await runCli(
      [
        'api-tokens',
        'create',
        'automation',
        '--token-stdin',
        '--scopes=project:write,build:write',
        `--principal=${PRINCIPAL_ID}`,
        '--expires-at=2027-01-02T03:04:05Z',
        '--idempotency-key=cli:token-create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readStdin: async (limitBytes) => {
          inputLimits.push(limitBytes);
          return input;
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(inputLimits).toEqual([69]);
    expect(input.every((byte) => byte === 0)).toBe(true);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/api-tokens`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:token-create' }),
        body: JSON.stringify({
          name: 'automation',
          token: API_TOKEN,
          scopes: ['project:write', 'build:write'],
          principalId: PRINCIPAL_ID,
          expiresAt: '2027-01-02T03:04:05.000Z',
        }),
      })
    );
    expect(output.stdout()).not.toContain(API_TOKEN);
    expect(output.stderr()).not.toContain(API_TOKEN);
  });

  it('revokes an API token idempotently without reading standard input', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ ...apiTokenResource(), revokedAt: '2026-07-27T01:00:00.000Z', replayed: false });
    };
    const output = capture();

    const exitCode = await runCli(
      ['api-tokens', 'revoke', API_TOKEN_ID, '--idempotency-key=cli:token-revoke', '--output=json'],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/api-tokens/${API_TOKEN_ID}`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:token-revoke' }),
        body: undefined,
      })
    );
    expect(output.stderr()).toBe('');
  });

  it.each([
    [['memberships', 'list'], `/organizations/${ORGANIZATION_ID}/memberships`, [membershipResource()]],
    [
      ['memberships', 'get', MEMBERSHIP_ID],
      `/organizations/${ORGANIZATION_ID}/memberships/${MEMBERSHIP_ID}`,
      membershipResource(),
    ],
  ] as const)('queries membership authority %#', async (command, path, response) => {
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

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stderr()).toBe('');
  });

  it('creates, changes, and revokes memberships through one optimistic lifecycle', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ ...membershipResource(), replayed: false }, calls.length === 1 ? 201 : 200);
    };
    const output = capture();
    const runtime = { ...output.runtime, environment: completeEnvironment(), fetch: fetcher };

    expect(
      await runCli(
        [
          'memberships',
          'create-service',
          'release automation',
          'member',
          '--idempotency-key=cli:membership-create',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'memberships',
          'change-role',
          MEMBERSHIP_ID,
          'restricted',
          '--expected-version=1',
          '--idempotency-key=cli:membership-role',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'memberships',
          'revoke',
          MEMBERSHIP_ID,
          '--expected-version=2',
          '--idempotency-key=cli:membership-revoke',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);

    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/memberships`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/memberships/${MEMBERSHIP_ID}/role`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/memberships/${MEMBERSHIP_ID}/revocation`,
    ]);
    expect(calls.map(([, init]) => init?.body)).toEqual([
      JSON.stringify({ name: 'release automation', role: 'member' }),
      JSON.stringify({ role: 'restricted', expectedVersion: 1 }),
      JSON.stringify({ expectedVersion: 2 }),
    ]);
    expect(calls.map(([, init]) => (init?.headers as Record<string, string>)['Idempotency-Key'])).toEqual([
      'cli:membership-create',
      'cli:membership-role',
      'cli:membership-revoke',
    ]);
    expect(output.stderr()).toBe('');
  });

  it.each([
    [
      ['resource-grants', 'list', MEMBERSHIP_ID],
      `/organizations/${ORGANIZATION_ID}/memberships/${MEMBERSHIP_ID}/resource-grants`,
      [resourceGrantResource()],
    ],
    [
      ['resource-grants', 'get', RESOURCE_GRANT_ID],
      `/organizations/${ORGANIZATION_ID}/resource-grants/${RESOURCE_GRANT_ID}`,
      resourceGrantResource(),
    ],
  ] as const)('queries Resource Grant history %#', async (command, path, response) => {
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

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stderr()).toBe('');
  });

  it('creates every closed Resource Grant scope and revokes by aggregate version', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ ...resourceGrantResource(), replayed: false }, calls.length <= 3 ? 201 : 200);
    };
    const output = capture();
    const runtime = { ...output.runtime, environment: completeEnvironment(), fetch: fetcher };
    const commands = [
      [
        'resource-grants',
        'create',
        MEMBERSHIP_ID,
        'project',
        PROJECT_ID,
        '--idempotency-key=cli:grant-project',
      ],
      [
        'resource-grants',
        'create',
        MEMBERSHIP_ID,
        'environment',
        PROJECT_ID,
        ENVIRONMENT_ID,
        '--idempotency-key=cli:grant-environment',
      ],
      ['resource-grants', 'create', MEMBERSHIP_ID, 'node', NODE_ID, '--idempotency-key=cli:grant-node'],
      [
        'resource-grants',
        'revoke',
        RESOURCE_GRANT_ID,
        '--expected-version=1',
        '--idempotency-key=cli:grant-revoke',
      ],
    ];
    for (const command of commands) {
      expect(await runCli(command, runtime)).toBe(ExitCode.Success);
    }

    expect(calls.map(([, init]) => init?.body)).toEqual([
      JSON.stringify({ scope: { kind: 'project', projectId: PROJECT_ID } }),
      JSON.stringify({
        scope: { kind: 'environment', projectId: PROJECT_ID, environmentId: ENVIRONMENT_ID },
      }),
      JSON.stringify({ scope: { kind: 'node', nodeId: NODE_ID } }),
      JSON.stringify({ expectedVersion: 1 }),
    ]);
    expect(calls.map(([, init]) => (init?.headers as Record<string, string>)['Idempotency-Key'])).toEqual([
      'cli:grant-project',
      'cli:grant-environment',
      'cli:grant-node',
      'cli:grant-revoke',
    ]);
    expect(output.stderr()).toBe('');
  });

  it('sanitizes a rejected mutation even if an upstream error echoes the credential', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 422,
          statusCode: 'UNPROCESSABLE_ENTITY',
          message: `invalid API token ${API_TOKEN}`,
          details: { token: API_TOKEN },
          requestId: '019c0000-0000-7000-8000-000000000019',
          timestamp: '2026-07-27T00:00:00.000Z',
        }),
        { status: 422 }
      );
    const output = capture();

    const exitCode = await runCli(
      [
        'api-tokens',
        'create',
        'automation',
        '--token-stdin',
        '--scopes=project:write',
        '--idempotency-key=cli:token-rejected',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readStdin: async () => new TextEncoder().encode(API_TOKEN),
      }
    );

    expect(exitCode).toBe(ExitCode.Api);
    expect(output.stderr()).toContain('API token mutation failed');
    expect(output.stderr()).not.toContain(API_TOKEN);
    expect(output.stdout()).toBe('');
  });

  it('rejects unsafe API token creation input before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const invalidUtf8 = new Uint8Array(68).fill(0x61);
    invalidUtf8[0] = 0xff;
    const cases: Array<{
      argv: string[];
      input?: Uint8Array;
      failure?: Error;
      message: string;
      hidden?: string;
    }> = [
      {
        argv: ['api-tokens', 'create', 'automation', '--scopes=project:write', '--idempotency-key=k'],
        message: '--token-stdin is required',
      },
      {
        argv: ['api-tokens', 'create', 'automation', '--token-stdin', '--idempotency-key=k'],
        input: new TextEncoder().encode(API_TOKEN),
        message: '--scopes is required',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=Project:write',
          '--idempotency-key=k',
        ],
        input: new TextEncoder().encode(API_TOKEN),
        message: 'scope must use bounded lowercase domain:action syntax',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=project:write',
          '--expires-at=tomorrow',
          '--idempotency-key=k',
        ],
        input: new TextEncoder().encode(API_TOKEN),
        message: 'expiry must be an RFC 3339 timestamp',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=project:write',
          '--expires-at=2027-02-30T03:04:05Z',
          '--idempotency-key=k',
        ],
        input: new TextEncoder().encode(API_TOKEN),
        message: 'expiry must be an RFC 3339 timestamp',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=project:write',
          '--idempotency-key=k',
        ],
        input: new TextEncoder().encode('short'),
        message: 'exactly 68 bytes',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=project:write',
          '--idempotency-key=k',
        ],
        input: invalidUtf8,
        message: 'valid UTF-8',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=project:write',
          '--idempotency-key=k',
        ],
        input: new Uint8Array(69),
        message: 'exactly 68 bytes',
      },
      {
        argv: [
          'api-tokens',
          'create',
          'automation',
          '--token-stdin',
          '--scopes=project:write',
          '--idempotency-key=k',
        ],
        failure: new Error('reader failed while handling do-not-leak'),
        message: 'unable to read API token credential from standard input',
        hidden: 'do-not-leak',
      },
      {
        argv: ['organizations', 'list', '--token-stdin'],
        message: '--token-stdin is valid only for API token creation',
      },
      {
        argv: ['organizations', 'list', '--scopes=project:write'],
        message: '--scopes is valid only for API token creation',
      },
      {
        argv: ['organizations', 'list', `--principal=${PRINCIPAL_ID}`],
        message: '--principal is valid only for API token creation',
      },
      {
        argv: ['memberships', 'change-role', MEMBERSHIP_ID, 'member', '--idempotency-key=k'],
        message: '--expected-version must be a positive safe integer for membership mutation',
      },
      {
        argv: ['memberships', 'create-service', 'automation', 'superuser', '--idempotency-key=k'],
        message: 'membership role must be owner, admin, member, or restricted',
      },
      {
        argv: ['resource-grants', 'create', MEMBERSHIP_ID, 'cluster', PROJECT_ID, '--idempotency-key=k'],
        message: 'Resource Grant scope kind must be project, environment, or node',
      },
      {
        argv: ['resource-grants', 'revoke', RESOURCE_GRANT_ID, '--idempotency-key=k'],
        message: '--expected-version must be a positive safe integer for Resource Grant mutation',
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
      expect(exitCode).toBe(ExitCode.Usage);
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
    A3S_CLOUD_TOKEN: 'caller-token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
  };
}

function apiTokenResource(): Record<string, unknown> {
  return {
    id: API_TOKEN_ID,
    organizationId: ORGANIZATION_ID,
    principalId: PRINCIPAL_ID,
    name: 'automation',
    scopes: ['project:write', 'build:write'],
    aggregateVersion: 1,
    createdAt: '2026-07-27T00:00:00.000Z',
    expiresAt: '2027-01-02T03:04:05.000Z',
    revokedAt: null,
  };
}

function membershipResource(): Record<string, unknown> {
  return {
    id: MEMBERSHIP_ID,
    organizationId: ORGANIZATION_ID,
    principalId: PRINCIPAL_ID,
    principalKind: 'service',
    principalName: 'release automation',
    principalAggregateVersion: 1,
    principalDisabledAt: null,
    role: 'member',
    aggregateVersion: 1,
    createdAt: '2026-08-07T00:00:00.000Z',
    updatedAt: '2026-08-07T00:00:00.000Z',
    revokedAt: null,
  };
}

function resourceGrantResource(): Record<string, unknown> {
  return {
    id: RESOURCE_GRANT_ID,
    organizationId: ORGANIZATION_ID,
    membershipId: MEMBERSHIP_ID,
    scope: { kind: 'project', projectId: PROJECT_ID },
    aggregateVersion: 1,
    createdAt: '2026-08-12T00:00:00.000Z',
    updatedAt: '2026-08-12T00:00:00.000Z',
    revokedAt: null,
  };
}

function addCredential(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => ({ ...(item as Record<string, unknown>), token: API_TOKEN }));
  }
  return { ...(value as Record<string, unknown>), token: API_TOKEN };
}
