import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000020';
const RECIPIENT_CONTACT_ID = '019c0000-0000-7000-8000-000000000030';
const ADDRESS = 'private.owner@example.test';
const PROOF = 'a3srcv1.opaque_payload.opaque_authenticator';

describe('a3s-cloud recipient contact commands', () => {
  it.each([
    [
      ['recipient-contacts', 'list'],
      `/organizations/${ORGANIZATION_ID}/recipient-contacts`,
      [{ ...recipientContactResource(), address: ADDRESS, proof: PROOF }],
    ],
    [
      ['recipient-contacts', 'get', RECIPIENT_CONTACT_ID],
      `/organizations/${ORGANIZATION_ID}/recipient-contacts/${RECIPIENT_CONTACT_ID}`,
      { ...recipientContactResource(), address: ADDRESS, proof: PROOF },
    ],
  ] as const)('renders only the redacted read projection %#', async (command, path, response) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli([...command, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(response);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stdout()).toContain('***@example.test');
    expect(output.stdout()).not.toContain(ADDRESS);
    expect(output.stdout()).not.toContain(PROOF);
    expect(output.stderr()).toBe('');
  });

  it('requests verification from bounded standard input and clears the mailbox bytes', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const limits: number[] = [];
    const bytes = new TextEncoder().encode(ADDRESS);
    const output = capture();
    const exitCode = await runCli(
      [
        'recipient-contacts',
        'request',
        '--address-stdin',
        '--idempotency-key=recipient:request',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ ...recipientContactResource(), address: ADDRESS, replayed: false }, 202);
        },
        readStdin: async (limitBytes) => {
          limits.push(limitBytes);
          return bytes;
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(limits).toEqual([255]);
    expect(bytes.every((byte) => byte === 0)).toBe(true);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/recipient-contacts`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'recipient:request' }),
        body: JSON.stringify({ address: ADDRESS }),
      })
    );
    expect(output.stdout()).not.toContain(ADDRESS);
    expect(output.stderr()).not.toContain(ADDRESS);
  });

  it('completes verification from bounded standard input and never prints the proof', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const limits: number[] = [];
    const bytes = new TextEncoder().encode(PROOF);
    const output = capture();
    const exitCode = await runCli(
      [
        'recipient-contacts',
        'verify',
        RECIPIENT_CONTACT_ID,
        '--proof-stdin',
        '--idempotency-key=recipient:verify',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({
            ...recipientContactResource(),
            status: 'verified',
            proof: PROOF,
            replayed: false,
          });
        },
        readStdin: async (limitBytes) => {
          limits.push(limitBytes);
          return bytes;
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(limits).toEqual([4097]);
    expect(bytes.every((byte) => byte === 0)).toBe(true);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/recipient-contacts/${RECIPIENT_CONTACT_ID}/verification`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'recipient:verify' }),
        body: JSON.stringify({ proof: PROOF }),
      })
    );
    expect(output.stdout()).not.toContain(PROOF);
    expect(output.stderr()).not.toContain(PROOF);
  });

  it('revokes with optimistic concurrency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'recipient-contacts',
        'revoke',
        RECIPIENT_CONTACT_ID,
        '--expected-version=2',
        '--idempotency-key=recipient:revoke',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ ...recipientContactResource(), status: 'revoked', replayed: false });
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/recipient-contacts/${RECIPIENT_CONTACT_ID}/revocation`
    );
    expect(calls[0]?.[1]?.body).toBe(JSON.stringify({ expectedVersion: 2 }));
    expect(output.stderr()).toBe('');
  });

  it('rejects unsafe input and sanitizes upstream failures without leakage', async () => {
    let called = false;
    const invalidAddressBytes = new TextEncoder().encode(' private.owner@example.test');
    const cases: Array<{ argv: string[]; bytes?: Uint8Array; message: string; hidden?: string }> = [
      {
        argv: ['recipient-contacts', 'request', '--idempotency-key=recipient:request'],
        message: '--address-stdin is required',
      },
      {
        argv: ['recipient-contacts', 'verify', RECIPIENT_CONTACT_ID, '--idempotency-key=recipient:verify'],
        message: '--proof-stdin is required',
      },
      {
        argv: ['recipient-contacts', 'request', '--address-stdin', '--idempotency-key=recipient:request'],
        bytes: invalidAddressBytes,
        message: 'bounded canonical ASCII mailbox',
        hidden: 'private.owner@example.test',
      },
      {
        argv: [
          'recipient-contacts',
          'verify',
          RECIPIENT_CONTACT_ID,
          '--proof-stdin',
          '--idempotency-key=recipient:verify',
        ],
        bytes: new TextEncoder().encode('private-invalid-proof'),
        message: 'recipient contact proof is invalid',
        hidden: 'private-invalid-proof',
      },
      {
        argv: ['organizations', 'list', '--address-stdin'],
        message: '--address-stdin is valid only',
      },
      {
        argv: ['organizations', 'list', '--proof-stdin'],
        message: '--proof-stdin is valid only',
      },
      {
        argv: ['recipient-contacts', 'revoke', RECIPIENT_CONTACT_ID, '--idempotency-key=recipient:revoke'],
        message: '--expected-version must be a positive safe integer for recipient contact mutation',
      },
    ];
    for (const testCase of cases) {
      const output = capture();
      const exitCode = await runCli(testCase.argv, {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async () => {
          called = true;
          return envelope({});
        },
        readStdin: async () => testCase.bytes ?? new Uint8Array(),
      });
      expect(exitCode).toBe(ExitCode.Usage);
      expect(output.stderr()).toContain(testCase.message);
      if (testCase.hidden) {
        expect(output.stderr()).not.toContain(testCase.hidden);
      }
    }
    expect(invalidAddressBytes.every((byte) => byte === 0)).toBe(true);
    expect(called).toBe(false);

    const output = capture();
    const exitCode = await runCli(
      [
        'recipient-contacts',
        'verify',
        RECIPIENT_CONTACT_ID,
        '--proof-stdin',
        '--idempotency-key=recipient:verify-failure',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readStdin: async () => new TextEncoder().encode(PROOF),
        fetch: async () =>
          new Response(
            JSON.stringify({
              code: 403,
              statusCode: 'FORBIDDEN',
              message: `rejected ${PROOF}`,
              details: { proof: PROOF },
              requestId: '019c0000-0000-7000-8000-000000000010',
              timestamp: '2026-08-23T00:00:00.000Z',
            }),
            { status: 403 }
          ),
      }
    );
    expect(exitCode).toBe(ExitCode.Authentication);
    expect(output.stderr()).toContain('recipient contact mutation failed');
    expect(output.stderr()).not.toContain(PROOF);
  });
});

function recipientContactResource(): Record<string, unknown> {
  return {
    id: RECIPIENT_CONTACT_ID,
    principalId: PRINCIPAL_ID,
    addressDigest: `sha256:${'a'.repeat(64)}`,
    addressHint: '***@example.test',
    aggregateVersion: 1,
    status: 'pending',
    createdAt: '2026-08-23T00:00:00.000Z',
    updatedAt: '2026-08-23T00:00:00.000Z',
    verifiedAt: null,
    revokedAt: null,
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-23T00:00:00.000Z',
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
