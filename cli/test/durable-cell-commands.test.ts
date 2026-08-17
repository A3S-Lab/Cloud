import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const APPLICATION_ID = '019c0000-0000-7000-8000-000000000004';
const REVISION_ID = '019c0000-0000-7000-8000-000000000005';
const GATEWAY_SCOPE_ID = '019c0000-0000-7000-8000-000000000006';
const DOMAIN_CLAIM_ID = '019c0000-0000-7000-8000-000000000007';
const WORKLOAD_ID = '019c0000-0000-7000-8000-000000000008';
const DEPLOYMENT_ID = '019c0000-0000-7000-8000-000000000009';
const OPERATION_ID = '019c0000-0000-7000-8000-00000000000a';
const ROUTE_ID = '019c0000-0000-7000-8000-00000000000b';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-00000000000c';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const APPLICATION_ACL = 'durable_cell_application { schema = "cloud.durable-cell.application.v1" }\n';
const SERVICE_PROFILE_ACL = 'durable_cell_service { schema = "cloud.durable-cell.service.v1" }\n';
const STORAGE_PROVIDER_PROFILE_ACL =
  'object_namespace_provider "s3_compatible" { schema = "cloud.object-namespace.provider-profile.v1" }\n';
const PROVIDER_WORKLOAD_ACL = 'version = 1\nworkload "durable-cell-provider" {}\n';
const STORAGE_BINDING_ACL = 'durable_cell_deployment { schema = "cloud.durable-cell.deployment.v1" }\n';
const BASE =
  `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}` +
  '/durable-cell-applications';

describe('a3s-cloud Durable Cell commands', () => {
  it.each([
    [['durable-cell-applications', 'list'], `${BASE}?limit=50`, [application()]],
    [['durable-cell-applications', 'get', APPLICATION_ID], `${BASE}/${APPLICATION_ID}`, record()],
    [
      ['durable-cell-revisions', 'list', APPLICATION_ID],
      `${BASE}/${APPLICATION_ID}/revisions?limit=50`,
      [revision()],
    ],
    [
      ['durable-cell-revisions', 'get', APPLICATION_ID, REVISION_ID],
      `${BASE}/${APPLICATION_ID}/revisions/${REVISION_ID}`,
      revision(),
    ],
  ] as const)('reads existing Durable Cells authority %#', async (argv, path, data) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli([...argv, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(data);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      action: 'create',
      argv: [
        'durable-cell-applications',
        'create',
        'Counter cells',
        '--file=application.acl',
        '--idempotency-key=cli:durable-cell:create',
      ],
      path: BASE,
      body: { name: 'Counter cells', definitionAcl: APPLICATION_ACL },
    },
    {
      action: 'revise',
      argv: [
        'durable-cell-applications',
        'revise',
        APPLICATION_ID,
        '--expected-version=1',
        '--file=application.acl',
        '--idempotency-key=cli:durable-cell:revise',
      ],
      path: `${BASE}/${APPLICATION_ID}/revisions`,
      body: { expectedVersion: 1, definitionAcl: APPLICATION_ACL },
    },
    {
      action: 'start',
      argv: [
        'durable-cell-applications',
        'start',
        APPLICATION_ID,
        '--expected-version=2',
        '--idempotency-key=cli:durable-cell:start',
      ],
      path: `${BASE}/${APPLICATION_ID}/start`,
      body: { expectedVersion: 2 },
    },
    {
      action: 'stop',
      argv: [
        'durable-cell-applications',
        'stop',
        APPLICATION_ID,
        '--expected-version=3',
        '--idempotency-key=cli:durable-cell:stop',
      ],
      path: `${BASE}/${APPLICATION_ID}/stop`,
      body: { expectedVersion: 3 },
    },
  ])('executes the $action application mutation through one client', async ({ argv, path, body }) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli([...argv, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async (path) => {
        expect(path).toBe('application.acl');
        return new TextEncoder().encode(APPLICATION_ACL);
      },
      fetch: async (...args) => {
        calls.push(args);
        return envelope({ record: record(), replayed: false }, 201);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(body),
        headers: expect.objectContaining({ 'Idempotency-Key': `cli:durable-cell:${argv[1]}` }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('deploys one exact revision from four bounded ACL files', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const readPaths: string[] = [];
    const output = capture();
    const files = new Map([
      ['service.acl', SERVICE_PROFILE_ACL],
      ['storage-provider.acl', STORAGE_PROVIDER_PROFILE_ACL],
      ['provider.acl', PROVIDER_WORKLOAD_ACL],
      ['storage.acl', STORAGE_BINDING_ACL],
    ]);
    const exitCode = await runCli(
      [
        'durable-cell-deployments',
        'create',
        APPLICATION_ID,
        REVISION_ID,
        '--service-profile-file=service.acl',
        '--storage-provider-profile-file=storage-provider.acl',
        '--provider-workload-file=provider.acl',
        '--storage-binding-file=storage.acl',
        '--idempotency-key=cli:durable-cell:deploy',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          readPaths.push(path);
          return new TextEncoder().encode(files.get(path) ?? '');
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope(deploymentResult(), 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(readPaths.sort()).toEqual(['provider.acl', 'service.acl', 'storage-provider.acl', 'storage.acl']);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1${BASE}/${APPLICATION_ID}/revisions/${REVISION_ID}/deployments`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          serviceProfileAcl: SERVICE_PROFILE_ACL,
          storageProviderProfileAcl: STORAGE_PROVIDER_PROFILE_ACL,
          providerWorkloadAcl: PROVIDER_WORKLOAD_ACL,
          storageBindingAcl: STORAGE_BINDING_ACL,
        }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:durable-cell:deploy' }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('publishes only the profile-selected public route through Edge', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'durable-cell-routes',
        'publish',
        APPLICATION_ID,
        REVISION_ID,
        GATEWAY_SCOPE_ID,
        DOMAIN_CLAIM_ID,
        'Cells.Example.Test',
        '/',
        '--service-profile-file=service.acl',
        '--idempotency-key=cli:durable-cell:route',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('service.acl');
          return new TextEncoder().encode(SERVICE_PROFILE_ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope(routePublicationResult(), 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1${BASE}/${APPLICATION_ID}/revisions/${REVISION_ID}/routes`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({
          serviceProfileAcl: SERVICE_PROFILE_ACL,
          gatewayScopeId: GATEWAY_SCOPE_ID,
          domainClaimId: DOMAIN_CLAIM_ID,
          hostname: 'cells.example.test',
          pathPrefix: '/',
        }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:durable-cell:route' }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects missing, oversized, versionless, and misplaced inputs before transport', async () => {
    let called = false;
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async () => {
        called = true;
        return envelope({});
      },
    };

    expect(
      await runCli(
        [
          'durable-cell-deployments',
          'create',
          APPLICATION_ID,
          REVISION_ID,
          '--service-profile-file=service.acl',
          '--storage-provider-profile-file=storage-provider.acl',
          '--provider-workload-file=provider.acl',
          '--idempotency-key=cli:durable-cell:missing-storage',
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('--storage-binding-file is required');

    expect(
      await runCli(
        [
          'durable-cell-deployments',
          'create',
          APPLICATION_ID,
          REVISION_ID,
          '--service-profile-file=service.acl',
          '--storage-provider-profile-file=storage-provider.acl',
          '--provider-workload-file=provider.acl',
          '--storage-binding-file=storage.acl',
          '--idempotency-key=cli:durable-cell:oversized',
        ],
        {
          ...runtime,
          readFile: async (path) =>
            path === 'storage.acl'
              ? new Uint8Array(MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES + 1)
              : new TextEncoder().encode(
                  path === 'service.acl'
                    ? SERVICE_PROFILE_ACL
                    : path === 'storage-provider.acl'
                      ? STORAGE_PROVIDER_PROFILE_ACL
                      : PROVIDER_WORKLOAD_ACL
                ),
        }
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('Durable Cell storage-binding ACL must contain between');

    expect(
      await runCli(
        [
          'durable-cell-applications',
          'stop',
          APPLICATION_ID,
          '--idempotency-key=cli:durable-cell:unversioned',
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('--expected-version must be a positive safe integer');

    expect(await runCli(['workloads', 'list', '--service-profile-file=service.acl'], runtime)).toBe(
      ExitCode.Usage
    );
    expect(output.stderr()).toContain(
      '--service-profile-file is valid only for Durable Cell deployment or route publication'
    );
    expect(called).toBe(false);
  });
});

function application() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    applicationId: APPLICATION_ID,
    name: 'Counter cells',
    desiredState: 'running',
    currentRevisionId: REVISION_ID,
    currentRevisionNumber: 1,
    currentDefinitionDigest: DIGEST,
    aggregateVersion: 1,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-16T00:00:00.000Z',
    updatedAt: '2026-08-16T00:00:00.000Z',
  };
}

function revision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    applicationId: APPLICATION_ID,
    revisionId: REVISION_ID,
    revisionNumber: 1,
    parentRevisionId: null,
    parentDefinitionDigest: null,
    definitionSchema: 'cloud.durable-cell.application.v1',
    definitionAcl: APPLICATION_ACL,
    definitionDigest: DIGEST,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-16T00:00:00.000Z',
  };
}

function record() {
  return { application: application(), revision: revision() };
}

function correlation() {
  return {
    applicationId: APPLICATION_ID,
    applicationRevisionNumber: 1,
    workloadId: WORKLOAD_ID,
    deploymentId: DEPLOYMENT_ID,
    operationId: OPERATION_ID,
    providerArtifactDigest: DIGEST,
  };
}

function deploymentResult() {
  return { correlation: correlation(), workload: {}, replayed: false };
}

function routePublicationResult() {
  return {
    correlation: correlation(),
    publication: {
      route: {
        id: ROUTE_ID,
        hostname: 'cells.example.test',
        pathPrefix: '/',
        state: 'active',
      },
      certificate: {},
      replayed: false,
      commandReplayed: false,
    },
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-16T00:00:00.000Z',
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
