import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const ASSET_ID = '019c0000-0000-7000-8000-000000000021';
const RELEASE_ID = '019c0000-0000-7000-8000-000000000022';

describe('a3s-cloud Asset commands', () => {
  it.each([
    [['assets', 'list'], `/organizations/${ORGANIZATION_ID}/assets`, [asset()]],
    [['assets', 'get', ASSET_ID], `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}`, asset()],
    [
      ['asset-releases', 'list', ASSET_ID],
      `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}/releases`,
      [release()],
    ],
    [
      ['asset-releases', 'get', ASSET_ID, RELEASE_ID],
      `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}/releases/${RELEASE_ID}`,
      release(),
    ],
    [
      ['asset-releases', 'select', ASSET_ID, '2.0.0-alpha.1'],
      `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}/release-selection?version=2.0.0-alpha.1`,
      release(),
    ],
  ] as const)('queries the organization Asset catalog %#', async (command, path, response) => {
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
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      command: ['assets', 'create', 'catalog-agent', 'agent'],
      path: `/organizations/${ORGANIZATION_ID}/assets`,
      body: { name: 'catalog-agent', kind: 'agent' },
      response: { ...asset(), replayed: false },
    },
    {
      command: ['assets', 'archive', ASSET_ID],
      path: `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}/archive`,
      body: undefined,
      response: { ...asset(), state: 'archived', replayed: false },
    },
    {
      command: ['asset-releases', 'create', ASSET_ID, '1.0.0', 'A'.repeat(40)],
      path: `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}/releases`,
      body: { version: '1.0.0', commitSha: 'a'.repeat(40) },
      response: { ...release(), replayed: false },
    },
    {
      command: ['asset-releases', 'yank', ASSET_ID, RELEASE_ID],
      path: `/organizations/${ORGANIZATION_ID}/assets/${ASSET_ID}/releases/${RELEASE_ID}/yank`,
      body: undefined,
      response: { ...release(), state: 'yanked', replayed: false },
    },
  ])('executes one idempotent Asset mutation %#', async ({ command, path, body, response }) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(response, 201);
    };
    const output = capture();

    const exitCode = await runCli([...command, '--idempotency-key=cli:asset-1', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:asset-1' }),
        body: body === undefined ? undefined : JSON.stringify(body),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects a shortened release commit before transport', async () => {
    let called = false;
    const output = capture();
    const exitCode = await runCli(
      ['asset-releases', 'create', ASSET_ID, '1.0.0', 'abc123', '--idempotency-key=cli:asset-invalid'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Usage);
    expect(called).toBe(false);
    expect(output.stderr()).toContain('full 40- or 64-character hexadecimal ID');
  });
});

function asset() {
  return {
    organizationId: ORGANIZATION_ID,
    id: ASSET_ID,
    name: 'catalog-agent',
    kind: 'agent',
    state: 'active',
    aggregateVersion: 1,
    createdAt: '2026-08-04T00:00:00.000Z',
    updatedAt: '2026-08-04T00:00:00.000Z',
    archivedAt: null,
  };
}

function release() {
  return {
    organizationId: ORGANIZATION_ID,
    assetId: ASSET_ID,
    id: RELEASE_ID,
    version: '1.0.0',
    state: 'published',
    commitSha: 'a'.repeat(40),
    manifestDigest: `sha256:${'b'.repeat(64)}`,
    artifact: {
      kind: 'oci_service',
      digest: `sha256:${'c'.repeat(64)}`,
      mediaType: 'application/vnd.oci.image.manifest.v1+json',
      sizeBytes: 1024,
    },
    provenance: null,
    aggregateVersion: 2,
    createdAt: '2026-08-04T00:00:00.000Z',
    updatedAt: '2026-08-04T00:01:00.000Z',
    publishedAt: '2026-08-04T00:01:00.000Z',
    yankedAt: null,
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-04T00:00:00.000Z',
    }),
    { status }
  );
}

function completeEnvironment(): Record<string, string> {
  return {
    A3S_CLOUD_TOKEN: `a3s_${'a'.repeat(64)}`,
    A3S_CLOUD_URL: 'http://127.0.0.1:8080/api/v1',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
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
