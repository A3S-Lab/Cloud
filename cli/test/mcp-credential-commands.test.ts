import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019d0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019d0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019d0000-0000-7000-8000-000000000003';
const CREDENTIAL_ID = '019d0000-0000-7000-8000-000000000004';
const PREFIX = `a3s_mcp_${'a'.repeat(16)}`;
const SECRET = `a3s_mcp_${'a'.repeat(80)}`;
const COLLECTION =
  `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
  `/environments/${ENVIRONMENT_ID}/mcp-credentials`;

describe('a3s-cloud MCP credential commands', () => {
  it.each([
    [['mcp-credentials', 'list'], COLLECTION, [credential()]],
    [['mcp-credentials', 'get', CREDENTIAL_ID], `${COLLECTION}/${CREDENTIAL_ID}`, credential()],
  ] as const)('queries only credential metadata %#', async (command, path, response) => {
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
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'GET',
        headers: expect.objectContaining({
          'Cache-Control': 'no-store',
          Pragma: 'no-cache',
        }),
      })
    );
    expect(output.stdout()).toContain(PREFIX);
    expect(output.stdout()).not.toContain('secret');
    expect(output.stdout()).not.toContain('verifier');
    expect(output.stdout()).not.toContain('ciphertext');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      command: ['mcp-credentials', 'issue'],
      path: COLLECTION,
      idempotencyKey: 'cli:mcp-issue',
      output: 'table',
    },
    {
      command: ['mcp-credentials', 'rotate', CREDENTIAL_ID],
      path: `${COLLECTION}/${CREDENTIAL_ID}/rotate`,
      idempotencyKey: 'cli:mcp-rotate',
      output: 'json',
    },
  ] as const)('fails closed if a mutation response contains internal material %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(
        {
          ...credential(),
          secret: SECRET,
          replayed: false,
          verifierHash: 'must-not-cross-client-boundary',
          ciphertext: 'must-not-cross-client-boundary',
        },
        testCase.command[1] === 'issue' ? 201 : 200
      );
    };
    const output = capture();

    const exitCode = await runCli(
      [
        ...testCase.command,
        '--expires-at=2027-01-02T03:04:05Z',
        `--idempotency-key=${testCase.idempotencyKey}`,
        `--output=${testCase.output}`,
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      }
    );

    // Forbidden server fields make the typed client fail closed before output.
    expect(exitCode).toBe(ExitCode.Transport);
    expect(output.stdout()).toBe('');
    expect(output.stderr()).not.toContain('must-not-cross-client-boundary');
    expect(output.stderr()).not.toContain(SECRET);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Cache-Control': 'no-store',
          'Idempotency-Key': testCase.idempotencyKey,
          Pragma: 'no-cache',
        }),
        body: JSON.stringify({ expiresAt: '2027-01-02T03:04:05.000Z' }),
      })
    );
  });

  it.each([
    {
      command: ['mcp-credentials', 'issue'],
      path: COLLECTION,
      idempotencyKey: 'cli:mcp-issue-clean',
      output: 'table',
    },
    {
      command: ['mcp-credentials', 'rotate', CREDENTIAL_ID],
      path: `${COLLECTION}/${CREDENTIAL_ID}/rotate`,
      idempotencyKey: 'cli:mcp-rotate-clean',
      output: 'json',
    },
  ] as const)('emits the full one-time secret only for a valid delivery %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(
        { ...credential(), secret: SECRET, replayed: false },
        testCase.command[1] === 'issue' ? 201 : 200
      );
    };
    const output = capture();

    const exitCode = await runCli(
      [
        ...testCase.command,
        '--expires-at=2027-01-02T03:04:05Z',
        `--idempotency-key=${testCase.idempotencyKey}`,
        `--output=${testCase.output}`,
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(output.stdout()).toContain(SECRET);
    expect(output.stdout()).not.toContain('verifier');
    expect(output.stdout()).not.toContain('ciphertext');
    expect(output.stderr()).toBe('');
  });

  it('revokes without accepting or displaying credential material', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({
        ...credential(),
        revokedAt: '2026-07-30T01:00:00.000Z',
        replayed: false,
      });
    };
    const output = capture();

    const exitCode = await runCli(
      ['mcp-credentials', 'revoke', CREDENTIAL_ID, '--idempotency-key=cli:mcp-revoke', '--output=json'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${COLLECTION}/${CREDENTIAL_ID}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'DELETE',
        headers: expect.objectContaining({
          'Cache-Control': 'no-store',
          'Idempotency-Key': 'cli:mcp-revoke',
          Pragma: 'no-cache',
        }),
        body: undefined,
      })
    );
    expect(output.stdout()).not.toContain('secret');
    expect(output.stderr()).toBe('');
  });

  it('sanitizes a rejected mutation even if an upstream error echoes credential material', async () => {
    const fetcher: CloudFetch = async () =>
      errorEnvelope(422, 'UNPROCESSABLE_ENTITY', `invalid credential ${SECRET}`, {
        secret: SECRET,
      });
    const output = capture();

    const exitCode = await runCli(
      [
        'mcp-credentials',
        'issue',
        '--expires-at=2027-01-02T03:04:05Z',
        '--idempotency-key=cli:mcp-rejected',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      }
    );

    expect(exitCode).toBe(ExitCode.Api);
    expect(output.stderr()).toContain('MCP credential mutation failed');
    expect(output.stderr()).not.toContain(SECRET);
    expect(output.stdout()).toBe('');
  });

  it('rejects unsafe command inputs before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const cases = [
      {
        argv: ['mcp-credentials', 'issue', '--idempotency-key=cli:mcp-missing-expiry'],
        message: '--expires-at is required for MCP credential',
      },
      {
        argv: [
          'mcp-credentials',
          'issue',
          '--expires-at=tomorrow',
          '--idempotency-key=cli:mcp-invalid-expiry',
        ],
        message: 'MCP credential expiry must be an RFC 3339 timestamp',
      },
      {
        argv: [
          'mcp-credentials',
          'rotate',
          'not-a-uuid',
          '--expires-at=2027-01-02T03:04:05Z',
          '--idempotency-key=cli:mcp-invalid-id',
        ],
        message: 'MCP credential ID must be a UUID',
      },
      {
        argv: ['organizations', 'list', '--expires-at=2027-01-02T03:04:05Z'],
        message: '--expires-at is valid only',
      },
    ];

    for (const testCase of cases) {
      const output = capture();
      const exitCode = await runCli(testCase.argv, {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      });
      expect(exitCode).toBe(ExitCode.Usage);
      expect(output.stderr()).toContain(testCase.message);
      expect(output.stdout()).toBe('');
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
      requestId: '019d0000-0000-7000-8000-000000000005',
      timestamp: '2026-07-30T00:00:00.000Z',
    }),
    {
      status,
      headers: {
        'cache-control': 'no-store',
        'content-type': 'application/json',
        pragma: 'no-cache',
      },
    }
  );
}

function errorEnvelope(
  status: number,
  statusCode: string,
  message: string,
  details: Record<string, unknown>
): Response {
  return new Response(
    JSON.stringify({
      code: status,
      statusCode,
      message,
      details,
      requestId: '019d0000-0000-7000-8000-000000000005',
      timestamp: '2026-07-30T00:00:00.000Z',
    }),
    {
      status,
      headers: {
        'cache-control': 'no-store',
        'content-type': 'application/json',
        pragma: 'no-cache',
      },
    }
  );
}

function credential(): Record<string, unknown> {
  return {
    id: CREDENTIAL_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    prefix: PREFIX,
    generation: 1,
    aggregateVersion: 1,
    expiresAt: '2027-01-02T03:04:05.000Z',
    createdAt: '2026-07-30T00:00:00.000Z',
    updatedAt: '2026-07-30T00:00:00.000Z',
    revokedAt: null,
  };
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
    A3S_CLOUD_URL: 'http://127.0.0.1:8080/api/v1',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
    A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
  };
}
