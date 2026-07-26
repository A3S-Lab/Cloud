import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const ENROLLMENT_TOKEN_ID = '019c0000-0000-7000-8000-000000000021';
const ENROLLMENT_TOKEN = `a3sn_${'b'.repeat(64)}`;
const AGENT_URL =
  'https://releases.example.test/a3s-cloud-node-agent/0.1.0/a3s-cloud-node-agent-linux-x86_64';
const AGENT_SHA256 = 'c'.repeat(64);

describe('a3s-cloud node bootstrap', () => {
  it('issues a bounded stdin-only credential and prints a checksum-verified installation invocation', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const inputLimits: number[] = [];
    const input = new TextEncoder().encode(ENROLLMENT_TOKEN);
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(
        {
          ...enrollmentTokenResource(),
          token: ENROLLMENT_TOKEN,
          replayed: false,
        },
        201
      );
    };
    const output = capture();

    const exitCode = await runCli(
      [
        'nodes',
        'bootstrap',
        'worker-1',
        '--enrollment-token-stdin',
        '--expires-at=2026-07-27T01:15:00Z',
        `--agent-release-url=${AGENT_URL}`,
        `--agent-release-sha256=${AGENT_SHA256}`,
        '--node-config=/etc/a3s-cloud/node.acl',
        '--idempotency-key=fleet:bootstrap:worker-1',
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
    expect(inputLimits).toEqual([70]);
    expect(input.every((byte) => byte === 0)).toBe(true);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/enrollment-tokens`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'fleet:bootstrap:worker-1' }),
        body: JSON.stringify({
          name: 'worker-1',
          token: ENROLLMENT_TOKEN,
          expiresAt: '2026-07-27T01:15:00.000Z',
        }),
      })
    );

    const result = JSON.parse(output.stdout()) as Record<string, unknown>;
    const invocation = result.installationInvocation as string;
    expect(result).toEqual(
      expect.objectContaining({
        id: ENROLLMENT_TOKEN_ID,
        organizationId: ORGANIZATION_ID,
        name: 'worker-1',
        expiresAt: '2026-07-27T01:15:00.000Z',
        replayed: false,
      })
    );
    expect(result).not.toHaveProperty('token');
    expect(invocation).toContain("curl --fail --location --proto '=https' --tlsv1.2");
    expect(invocation).toContain(AGENT_URL);
    expect(invocation).toContain(AGENT_SHA256);
    expect(invocation).toContain('sha256sum --check --strict -');
    expect(invocation).toContain(
      'sudo install --mode=0755 "$staging" \'/usr/local/bin/a3s-cloud-node-agent\''
    );
    expect(invocation).toContain("read -r -s -p 'Enrollment credential: ' A3S_CLOUD_ENROLLMENT_TOKEN");
    expect(invocation).toContain("exec '/usr/local/bin/a3s-cloud-node-agent' '/etc/a3s-cloud/node.acl'");
    expect(output.stdout()).not.toContain(ENROLLMENT_TOKEN);
    expect(output.stderr()).toBe('');
  });

  it('sanitizes a rejected issuance even when the API echoes the enrollment credential', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 422,
          statusCode: 'UNPROCESSABLE_ENTITY',
          message: `invalid enrollment credential ${ENROLLMENT_TOKEN}`,
          details: { token: ENROLLMENT_TOKEN },
          requestId: ENROLLMENT_TOKEN,
          timestamp: '2026-07-27T00:00:00.000Z',
        }),
        { status: 422 }
      );
    const output = capture();

    const exitCode = await runCli(bootstrapArguments(), {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
      readStdin: async () => new TextEncoder().encode(ENROLLMENT_TOKEN),
    });

    expect(exitCode).toBe(ExitCode.Api);
    expect(output.stderr()).toContain('node enrollment credential issuance failed');
    expect(output.stderr()).not.toContain(ENROLLMENT_TOKEN);
    expect(output.stdout()).toBe('');
  });

  it('rejects unsafe bootstrap input before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const invalidUtf8 = new Uint8Array(69).fill(0x61);
    invalidUtf8[0] = 0xff;
    const cases: Array<{
      argv: string[];
      input?: Uint8Array;
      message: string;
    }> = [
      {
        argv: bootstrapArguments().filter((value) => value !== '--enrollment-token-stdin'),
        message: '--enrollment-token-stdin is required',
      },
      {
        argv: bootstrapArguments().filter((value) => !value.startsWith('--expires-at=')),
        message: '--expires-at is required',
      },
      {
        argv: replaceOption('--expires-at=', '--expires-at=2026-02-30T01:15:00Z'),
        message: 'expiry must be an RFC 3339 timestamp',
      },
      {
        argv: replaceOption('--agent-release-url=', '--agent-release-url=http://releases.example.test/agent'),
        message: 'release URL must use HTTPS',
      },
      {
        argv: replaceOption('--agent-release-sha256=', `--agent-release-sha256=${'A'.repeat(64)}`),
        message: 'release SHA-256 must contain 64 lowercase hex digits',
      },
      {
        argv: replaceOption('--node-config=', '--node-config=node.toml'),
        message: 'node config must be an absolute .acl path',
      },
      {
        argv: bootstrapArguments(),
        input: new TextEncoder().encode('short'),
        message: 'exactly 69 bytes',
      },
      {
        argv: bootstrapArguments(),
        input: invalidUtf8,
        message: 'valid UTF-8',
      },
      {
        argv: bootstrapArguments(),
        input: new Uint8Array(70),
        message: 'exactly 69 bytes',
      },
      {
        argv: ['organizations', 'list', '--enrollment-token-stdin'],
        message: '--enrollment-token-stdin is valid only for nodes bootstrap',
      },
      {
        argv: ['nodes', 'list', `--agent-release-url=${AGENT_URL}`],
        message: 'agent release and node config options are valid only for nodes bootstrap',
      },
    ];

    for (const testCase of cases) {
      const output = capture();
      const exitCode = await runCli(testCase.argv, {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readStdin: async () => testCase.input ?? new TextEncoder().encode(ENROLLMENT_TOKEN),
      });
      expect(exitCode).toBe(ExitCode.Usage);
      expect(output.stderr()).toContain(testCase.message);
      expect(output.stderr()).not.toContain(ENROLLMENT_TOKEN);
    }
    expect(called).toBe(false);
  });
});

function bootstrapArguments(): string[] {
  return [
    'nodes',
    'bootstrap',
    'worker-1',
    '--enrollment-token-stdin',
    '--expires-at=2026-07-27T01:15:00Z',
    `--agent-release-url=${AGENT_URL}`,
    `--agent-release-sha256=${AGENT_SHA256}`,
    '--node-config=/etc/a3s-cloud/node.acl',
    '--idempotency-key=fleet:bootstrap:worker-1',
  ];
}

function replaceOption(prefix: string, replacement: string): string[] {
  return bootstrapArguments().map((value) => (value.startsWith(prefix) ? replacement : value));
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000020',
      timestamp: '2026-07-27T00:00:00.000Z',
    }),
    { status }
  );
}

function enrollmentTokenResource(): Record<string, unknown> {
  return {
    id: ENROLLMENT_TOKEN_ID,
    organizationId: ORGANIZATION_ID,
    name: 'worker-1',
    aggregateVersion: 1,
    createdAt: '2026-07-27T00:00:00.000Z',
    expiresAt: '2026-07-27T01:15:00.000Z',
    usedAt: null,
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
  };
}
