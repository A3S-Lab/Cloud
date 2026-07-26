import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-07-26T00:00:00.000Z',
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

describe('a3s-cloud CLI', () => {
  it('shows resolved context without exposing the API token', async () => {
    const output = capture();

    const exitCode = await runCli(['context', 'show', '--output=json'], {
      ...output.runtime,
      environment: {
        A3S_CLOUD_TOKEN: 'a3s_secret',
        A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
      },
    });

    expect(exitCode).toBe(0);
    expect(output.stdout()).toContain('"tokenConfigured": true');
    expect(output.stdout()).not.toContain('a3s_secret');
    expect(output.stderr()).toBe('');
  });

  it('lists organizations through the typed client and sanitizes table cells', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope([
        {
          id: ORGANIZATION_ID,
          name: '\u001b[31mOperations\nTeam',
          aggregateVersion: 1,
          createdAt: '2026-07-26T00:00:00.000Z',
        },
      ]);
    };
    const output = capture();

    const exitCode = await runCli(['organizations', 'list'], {
      ...output.runtime,
      environment: { A3S_CLOUD_TOKEN: 'a3s_secret' },
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe('http://127.0.0.1:8080/api/v1/organizations');
    expect(calls[0]?.[1]?.headers).toEqual(expect.objectContaining({ Authorization: 'Bearer a3s_secret' }));
    expect(output.stdout()).toContain('[31mOperations Team');
    expect(output.stdout()).not.toContain('\u001b');
  });

  it('lists nodes as stable JSON in the selected organization', async () => {
    const fetcher: CloudFetch = async () => envelope([]);
    const output = capture();

    const exitCode = await runCli(['nodes', 'list', '--output', 'json'], {
      ...output.runtime,
      environment: {
        A3S_CLOUD_TOKEN: 'token',
        A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
      },
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(output.stdout()).toBe('[]\n');
  });

  it('fails before the network when required context is absent', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope([]);
    };
    const output = capture();

    const exitCode = await runCli(['projects', 'list', '--output=table'], {
      ...output.runtime,
      environment: { A3S_CLOUD_TOKEN: 'token', A3S_CLOUD_OUTPUT: 'json' },
      fetch: fetcher,
    });

    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain('organization ID is required');
    expect(output.stderr()).not.toContain('"error"');
  });

  it('maps authentication failures to a redacted JSON error and stable exit code', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 401,
          statusCode: 'UNAUTHORIZED',
          message: 'invalid credential a3s_secret',
          details: { token: 'a3s_secret', reason: 'revoked' },
          requestId: '019c0000-0000-7000-8000-000000000011',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 401 }
      );
    const output = capture();

    const exitCode = await runCli(['organizations', 'list', '--output=json'], {
      ...output.runtime,
      environment: { A3S_CLOUD_TOKEN: 'a3s_secret' },
      fetch: fetcher,
    });

    expect(exitCode).toBe(3);
    expect(output.stderr()).toContain('"statusCode": "UNAUTHORIZED"');
    expect(output.stderr()).toContain('"reason": "revoked"');
    expect(output.stderr()).not.toContain('a3s_secret');
  });

  it('maps transport failures without leaking implementation errors or tokens', async () => {
    const fetcher: CloudFetch = async () => {
      throw new Error('dial failed with token a3s_secret');
    };
    const output = capture();

    const exitCode = await runCli(['organizations', 'list'], {
      ...output.runtime,
      environment: { A3S_CLOUD_TOKEN: 'a3s_secret' },
      fetch: fetcher,
    });

    expect(exitCode).toBe(7);
    expect(output.stderr()).toBe('NETWORK_ERROR: Cloud API request failed\n');
    expect(output.stderr()).not.toContain('a3s_secret');
  });

  it('never accepts a token through process arguments', async () => {
    const output = capture();

    const exitCode = await runCli(['--token=a3s_secret', 'organizations', 'list'], output.runtime);

    expect(exitCode).toBe(2);
    expect(output.stderr()).toContain('accepted only through A3S_CLOUD_TOKEN');
    expect(output.stderr()).not.toContain('a3s_secret');
  });
});
