import { describe, expect, it } from 'bun:test';
import type { CloudFetch, SearchResult } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const WORKLOAD_ID = '019c0000-0000-7000-8000-000000000004';

const SEARCH_RESULTS: SearchResult[] = [
  {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    workloadId: WORKLOAD_ID,
    kind: 'workload',
    id: WORKLOAD_ID,
    title: 'cloud-worker',
    description: 'Production worker workload',
    state: 'running',
    href: `#/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workloads/${WORKLOAD_ID}`,
    updatedAt: '2026-07-27T01:00:00.000Z',
  },
];

function envelope(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-07-27T01:01:00.000Z',
    }),
    { status: 200 }
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

function searchEnvironment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
  };
}

describe('search resources command', () => {
  it('queries the tenant-authorized API with a normalized query and explicit limit', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(SEARCH_RESULTS);
    };
    const output = capture();

    const exitCode = await runCli(['search', 'resources', '  Cloud worker  ', '--limit=25'], {
      ...output.runtime,
      environment: searchEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/search?q=Cloud+worker&limit=25`
    );
    expect(calls[0]?.[1]?.headers).toEqual(expect.objectContaining({ Authorization: 'Bearer token' }));
    expect(output.stdout()).toContain('KIND');
    expect(output.stdout()).toContain('TITLE');
    expect(output.stdout()).toContain('DESCRIPTION');
    expect(output.stdout()).toContain('HREF');
    expect(output.stdout()).toContain('cloud-worker');
    expect(output.stdout()).toContain('Production worker workload');
    expect(output.stderr()).toBe('');
  });

  it('uses the bounded default limit and preserves the typed result in JSON', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(SEARCH_RESULTS);
    };
    const output = capture();

    const exitCode = await runCli(['search', 'resources', 'worker', '--output=json'], {
      ...output.runtime,
      environment: searchEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/search?q=worker&limit=20`
    );
    expect(output.stdout()).toBe(`${JSON.stringify(SEARCH_RESULTS, null, 2)}\n`);
    expect(output.stderr()).toBe('');
  });

  it.each([
    ['', 'search query must contain 1 to 128 safe characters'],
    ['   ', 'search query must contain 1 to 128 safe characters'],
    ['line\nbreak', 'search query must contain 1 to 128 safe characters'],
    ['界'.repeat(129), 'search query must contain 1 to 128 safe characters'],
  ])('rejects an invalid query before transport %#', async (query, message) => {
    let called = false;
    const output = capture();

    const exitCode = await runCli(['search', 'resources', query], {
      ...output.runtime,
      environment: searchEnvironment(),
      fetch: async () => {
        called = true;
        return envelope([]);
      },
    });

    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain(message);
  });

  it.each([
    ['0', 'search result limit must be between 1 and 50'],
    ['51', 'search result limit must be between 1 and 50'],
    ['1.5', 'search result limit must be an integer'],
    ['many', 'search result limit must be an integer'],
  ])('rejects an invalid search limit before transport %#', async (limit, message) => {
    let called = false;
    const output = capture();

    const exitCode = await runCli(['search', 'resources', 'worker', `--limit=${limit}`], {
      ...output.runtime,
      environment: searchEnvironment(),
      fetch: async () => {
        called = true;
        return envelope([]);
      },
    });

    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain(message);
  });

  it.each([
    [
      { A3S_CLOUD_TOKEN: 'token' },
      'an organization ID is required through --organization or A3S_CLOUD_ORGANIZATION_ID',
    ],
    [{ A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID }, 'A3S_CLOUD_TOKEN is required for API commands'],
  ])('requires tenant and credential context before transport %#', async (environment, message) => {
    let called = false;
    const output = capture();

    const exitCode = await runCli(['search', 'resources', 'worker'], {
      ...output.runtime,
      environment,
      fetch: async () => {
        called = true;
        return envelope([]);
      },
    });

    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain(message);
  });

  it.each([
    [['search', 'resources', 'worker', '--cursor=v1:1'], 'cursor and stream options'],
    [['search', 'resources', 'worker', '--stream=stdout'], 'cursor and stream options'],
    [['search', 'resources', 'worker', '--idempotency-key=search:1'], 'valid only for mutation commands'],
    [['search', 'resources', 'worker', '--file=search.acl'], 'valid only for file-backed mutation'],
  ])('rejects a misplaced option before transport %#', async (argv, message) => {
    let called = false;
    const output = capture();

    const exitCode = await runCli(argv, {
      ...output.runtime,
      environment: searchEnvironment(),
      fetch: async () => {
        called = true;
        return envelope([]);
      },
    });

    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain(message);
  });
});
