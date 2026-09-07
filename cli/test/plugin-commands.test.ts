import { describe, expect, it } from 'bun:test';
import type { CloudFetch, PluginAssignment, PluginRegistry } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000011';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000012';
const REGISTRY_ID = '019c0000-0000-7000-8000-000000000002';
const HOST_ID = '019c0000-0000-7000-8000-000000000003';
const ASSIGNMENT_ID = '019c0000-0000-7000-8000-000000000004';
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
const ASSIGNMENT: PluginAssignment = {
  organizationId: ORGANIZATION_ID,
  projectId: PROJECT_ID,
  environmentId: ENVIRONMENT_ID,
  id: ASSIGNMENT_ID,
  registryId: REGISTRY_ID,
  targetHostId: HOST_ID,
  workspaceScope: {
    schema: 'a3s.use.plugin-managed-scope.v2',
    hostId: 'host:node-01',
    scopeKind: 'workspace',
    scopeId: 'workspace:research',
    authorityId: 'cloud:organization-01',
    fenceGeneration: 7,
    fenceDigest: `sha256:${'d'.repeat(64)}`,
  },
  packageId: 'a3s/registry-selftest',
  catalogRecordDigest: `sha256:${'a'.repeat(64)}`,
  version: '0.1.0',
  packageDigest: `sha256:${'b'.repeat(64)}`,
  manifestDigest: `sha256:${'c'.repeat(64)}`,
  selectedSurfaces: [{ kind: 'skill', id: 'selftest' }],
  policyDigest: `sha256:${'e'.repeat(64)}`,
  desiredState: 'enabled',
  assignmentGeneration: 1,
  aggregateVersion: 1,
  currentOperationId: null,
  createdAt: '2026-08-12T00:00:00.000Z',
  updatedAt: '2026-08-12T00:00:00.000Z',
};

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-12T00:01:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
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
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
    A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
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

  it('lists, gets, and sets environment-scoped plugin assignments', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const assignmentInput = {
      registryId: REGISTRY_ID,
      targetHostId: HOST_ID,
      workspaceScope: ASSIGNMENT.workspaceScope,
      selection: {
        packageId: ASSIGNMENT.packageId,
        catalogRecordDigest: ASSIGNMENT.catalogRecordDigest,
        version: ASSIGNMENT.version,
        packageDigest: ASSIGNMENT.packageDigest,
        manifestDigest: ASSIGNMENT.manifestDigest,
        selectedSurfaces: ASSIGNMENT.selectedSurfaces,
      },
      policyDigest: ASSIGNMENT.policyDigest,
      desiredState: ASSIGNMENT.desiredState,
    };
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const [input, init] = args;
      if (init?.method === 'PUT') {
        return envelope({ assignment: ASSIGNMENT, replayed: false }, 201);
      }
      return envelope(String(input).endsWith('/plugin-assignments') ? [ASSIGNMENT] : ASSIGNMENT);
    };
    const listed = capture();
    const fetched = capture();
    const set = capture();

    expect(
      await runCli(['plugin-assignments', 'list'], {
        ...listed.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(0);
    expect(
      await runCli(['plugin-assignments', 'get', ASSIGNMENT_ID, '--output=json'], {
        ...fetched.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(0);
    expect(
      await runCli(
        [
          'plugin-assignments',
          'set',
          '--file=assignment.json',
          '--idempotency-key=assign:1',
          '--output=json',
        ],
        {
          ...set.runtime,
          environment: environment(),
          fetch: fetcher,
          readFile: async () => new TextEncoder().encode(JSON.stringify(assignmentInput)),
        }
      )
    ).toBe(0);

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        body: init?.body,
        idempotencyKey: (init?.headers as Partial<Record<string, string>> | undefined)?.[
          'Idempotency-Key'
        ],
      }))
    ).toEqual([
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/plugin-assignments`,
        method: 'GET',
        body: undefined,
        idempotencyKey: undefined,
      },
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/plugin-assignments/${ASSIGNMENT_ID}`,
        method: 'GET',
        body: undefined,
        idempotencyKey: undefined,
      },
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/plugin-assignments`,
        method: 'PUT',
        body: JSON.stringify(assignmentInput),
        idempotencyKey: 'assign:1',
      },
    ]);
    expect(listed.stdout()).toContain('a3s/registry-selftest');
    expect(fetched.stdout()).toBe(`${JSON.stringify(ASSIGNMENT, null, 2)}\n`);
    expect(set.stdout()).toContain('"replayed": false');
    expect(listed.stderr()).toBe('');
    expect(fetched.stderr()).toBe('');
    expect(set.stderr()).toBe('');
  });

  it('gets and confirms organization-scoped plugin plan projections', async () => {
    const PROJECTION_ID = '019c0000-0000-7000-8000-000000000005';
    const projection = {
      organizationId: ORGANIZATION_ID,
      id: PROJECTION_ID,
      assignmentId: ASSIGNMENT_ID,
      operationId: '019c0000-0000-7000-8000-000000000006',
      assignmentGeneration: 1,
      useOperationId: 'install:acme-research:0001',
      planSchema: 'a3s.use.plugin-operation-plan.v4',
      planDigest: `sha256:${'a'.repeat(64)}`,
      expiresAt: '2026-08-12T00:10:00.000Z',
      action: 'install',
      rootPackageId: 'acme/research',
      rootPackageDigest: `sha256:${'b'.repeat(64)}`,
      rootManifestDigest: `sha256:${'c'.repeat(64)}`,
      authorityDecision: 'ask',
      authorityPolicyDigest: `sha256:${'d'.repeat(64)}`,
      impactDigest: `sha256:${'e'.repeat(64)}`,
      permissionEvidenceDigest: `sha256:${'f'.repeat(64)}`,
      providerEvidenceDigest: `sha256:${'1'.repeat(64)}`,
      confirmationDigest: null,
      terminalReason: null,
      awaitsConfirmation: true,
      createdAt: '2026-08-12T00:00:00.000Z',
      updatedAt: '2026-08-12T00:00:00.000Z',
    };
    const confirmationInput = {
      confirmation: {
        schema: 'a3s.use.plugin-operation-confirmation.v1',
        operationId: 'install:acme-research:0001',
        planDigest: projection.planDigest,
        confirmedBy: 'user',
        confirmedAtMs: 1,
      },
    };
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(projection);
    };
    const fetched = capture();
    const confirmed = capture();

    expect(
      await runCli(['plugin-plan-projections', 'get', PROJECTION_ID, '--output=json'], {
        ...fetched.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(0);
    expect(
      await runCli(
        [
          'plugin-plan-projections',
          'confirm',
          PROJECTION_ID,
          '--file=confirmation.json',
          '--idempotency-key=confirm:1',
          '--output=json',
        ],
        {
          ...confirmed.runtime,
          environment: environment(),
          fetch: fetcher,
          readFile: async () => new TextEncoder().encode(JSON.stringify(confirmationInput)),
        }
      )
    ).toBe(0);

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        body: init?.body,
        idempotencyKey: (init?.headers as Partial<Record<string, string>> | undefined)?.[
          'Idempotency-Key'
        ],
      }))
    ).toEqual([
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-plan-projections/${PROJECTION_ID}`,
        method: 'GET',
        body: undefined,
        idempotencyKey: undefined,
      },
      {
        input: `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/plugin-plan-projections/${PROJECTION_ID}/confirmation`,
        method: 'PUT',
        body: JSON.stringify(confirmationInput),
        idempotencyKey: 'confirm:1',
      },
    ]);
  });

  it('rejects missing or malformed assignment JSON before transport', async () => {
    let called = false;
    const missing = capture();
    const invalid = capture();
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };

    expect(
      await runCli(['plugin-assignments', 'set', '--idempotency-key=assign:1'], {
        ...missing.runtime,
        environment: environment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(missing.stderr()).toContain('--file with a valid plugin assignment JSON path is required');

    expect(
      await runCli(
        ['plugin-assignments', 'set', '--file=invalid.json', '--idempotency-key=assign:1'],
        {
          ...invalid.runtime,
          environment: environment(),
          fetch: fetcher,
          readFile: async () => new TextEncoder().encode('[]'),
        }
      )
    ).toBe(2);
    expect(invalid.stderr()).toContain('Plugin assignment input must contain registryId');
    expect(called).toBe(false);
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
