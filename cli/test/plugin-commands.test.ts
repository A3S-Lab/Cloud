import { describe, expect, it } from 'bun:test';
import type { CloudFetch, PluginRegistry } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const REGISTRY_ID = '019c0000-0000-7000-8000-000000000002';
const REGISTRY: PluginRegistry = {
  organizationId: ORGANIZATION_ID,
  id: REGISTRY_ID,
  name: 'Official',
  endpoint: 'https://registry.example.test/a3s',
  rootObjectRef: 'plugin-trust-roots/root.json',
  rootSha256: `sha256:${'a'.repeat(64)}`,
  rootVersion: 7,
  state: 'active',
  aggregateVersion: 1,
  createdAt: '2026-08-12T00:00:00.000Z',
  updatedAt: '2026-08-12T00:00:00.000Z',
};

function envelope(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-12T00:01:00.000Z',
    }),
    { status: 200, headers: { 'content-type': 'application/json' } }
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

function environment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
  };
}

describe('plugin catalog commands', () => {
  it('lists and gets only tenant-scoped Registry projections', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(String(args[0]).endsWith('/plugin-registries') ? [REGISTRY] : REGISTRY);
    };
    const listed = capture();
    const fetched = capture();

    expect(
      await runCli(['plugin-registries', 'list'], {
        ...listed.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(0);
    expect(
      await runCli(['plugin-registries', 'get', REGISTRY_ID, '--output=json'], {
        ...fetched.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(0);

    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-registries`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-registries/${REGISTRY_ID}`,
    ]);
    expect(listed.stdout()).toContain('ROOT SHA-256');
    expect(listed.stdout()).toContain('Official');
    expect(fetched.stdout()).toBe(`${JSON.stringify(REGISTRY, null, 2)}\n`);
    expect(listed.stderr()).toBe('');
    expect(fetched.stderr()).toBe('');
  });

  it('passes canonical A3S Use JSON through the four non-mutating POST queries', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ metadata: {}, packages: [] });
    };
    const searchRequest = {
      host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
      search: { query: 'a3s', limit: 20 },
    };
    const inspectRequest = {
      host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
      packageId: 'a3s/example',
    };
    const files: Readonly<Record<string, unknown>> = {
      'search.json': searchRequest,
      'inspect.json': inspectRequest,
    };
    const readFile = async (path: string) => new TextEncoder().encode(JSON.stringify(files[path]));

    for (const [action, file] of [
      ['search', 'search.json'],
      ['search-cached', 'search.json'],
      ['inspect', 'inspect.json'],
      ['inspect-cached', 'inspect.json'],
    ] as const) {
      const output = capture();
      expect(
        await runCli(['plugin-catalog', action, REGISTRY_ID, `--file=${file}`, '--output=json'], {
          ...output.runtime,
          environment: environment(),
          fetch: fetcher,
          readFile,
        })
      ).toBe(0);
      expect(output.stderr()).toBe('');
    }

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        body: init?.body,
        idempotencyKey: (init?.headers as Partial<Record<string, string>> | undefined)?.['Idempotency-Key'],
      }))
    ).toEqual([
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-registries/${REGISTRY_ID}/catalog/search`,
        method: 'POST',
        body: JSON.stringify(searchRequest),
        idempotencyKey: undefined,
      },
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-registries/${REGISTRY_ID}/catalog/cache/search`,
        method: 'POST',
        body: JSON.stringify(searchRequest),
        idempotencyKey: undefined,
      },
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-registries/${REGISTRY_ID}/catalog/inspect`,
        method: 'POST',
        body: JSON.stringify(inspectRequest),
        idempotencyKey: undefined,
      },
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-registries/${REGISTRY_ID}/catalog/cache/inspect`,
        method: 'POST',
        body: JSON.stringify(inspectRequest),
        idempotencyKey: undefined,
      },
    ]);
  });

  it('rejects missing or malformed catalog JSON before transport', async () => {
    let called = false;
    const missing = capture();
    const invalid = capture();
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };

    expect(
      await runCli(['plugin-catalog', 'search', REGISTRY_ID], {
        ...missing.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(missing.stderr()).toContain('--file with a valid A3S Use catalog request JSON path is required');

    expect(
      await runCli(['plugin-catalog', 'inspect', REGISTRY_ID, '--file=invalid.json'], {
        ...invalid.runtime,
        environment: environment(),
        fetch: fetcher,
        readFile: async () => new TextEncoder().encode('[]'),
      })
    ).toBe(2);
    expect(invalid.stderr()).toContain('Plugin catalog request must be a JSON object');
    expect(called).toBe(false);
  });
});
