import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const WORKLOAD_ID = '019c0000-0000-7000-8000-000000000004';
const REVISION_ID = '019c0000-0000-7000-8000-000000000005';
const ROUTE_ID = '019c0000-0000-7000-8000-000000000006';
const DEPLOYMENT_ID = '019c0000-0000-7000-8000-000000000007';
const BUILD_RUN_ID = '019c0000-0000-7000-8000-000000000008';
const NODE_ID = '019c0000-0000-7000-8000-000000000009';
const DOMAIN_CLAIM_ID = '019c0000-0000-7000-8000-000000000011';
const GATEWAY_SCOPE_ID = '019c0000-0000-7000-8000-000000000012';
const GATEWAY_NODE_ID = '019c0000-0000-7000-8000-000000000013';
const SOURCE_REVISION_ID = '019c0000-0000-7000-8000-000000000014';
const SOURCE_SUBSCRIPTION_ID = '019c0000-0000-7000-8000-000000000015';
const ASSET_ID = '019c0000-0000-7000-8000-000000000016';
const ASSET_RELEASE_ID = '019c0000-0000-7000-8000-000000000017';

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

  it('reports public platform, liveness, and readiness without requiring a token', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      if (path.endsWith('/platform')) {
        return envelope({ name: 'a3s-cloud', version: '0.1.0', role: 'api' });
      }
      return envelope({ status: 'up', checks: {} });
    };
    const output = capture();

    const exitCode = await runCli(['diagnostics', 'status', '--output=json'], {
      ...output.runtime,
      environment: {},
      fetch: fetcher,
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls.map(([input]) => input)).toEqual([
      'http://127.0.0.1:8080/api/v1/platform',
      'http://127.0.0.1:8080/api/v1/health/live',
      'http://127.0.0.1:8080/api/v1/health/ready',
    ]);
    for (const [, init] of calls) {
      expect(init?.headers).not.toHaveProperty('Authorization');
    }
    expect(output.stdout()).toContain('"liveness"');
    expect(output.stdout()).toContain('"readiness"');
    expect(output.stderr()).toBe('');
  });

  it('returns a stable unhealthy exit code while preserving down diagnostics', async () => {
    const fetcher: CloudFetch = async (input) => {
      const path = String(input);
      if (path.endsWith('/platform')) {
        return envelope({ name: 'a3s-cloud', version: '0.1.0', role: 'worker' });
      }
      if (path.endsWith('/health/live')) {
        return envelope({ status: 'up', checks: {} });
      }
      return envelope({ status: 'down', checks: { repositories: { status: 'down', details: {} } } }, 503);
    };
    const output = capture();

    const exitCode = await runCli(['diagnostics', 'status', '--output=json'], {
      ...output.runtime,
      environment: {},
      fetch: fetcher,
    });

    expect(exitCode).toBe(ExitCode.Unhealthy);
    expect(output.stdout()).toContain('"status": "down"');
    expect(output.stdout()).toContain('"repositories"');
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

  it.each([
    {
      command: ['organizations', 'create', 'Operations'],
      path: '/organizations',
      body: { name: 'Operations' },
      response: {
        id: ORGANIZATION_ID,
        name: 'Operations',
        aggregateVersion: 1,
        createdAt: '2026-07-27T00:00:00.000Z',
        replayed: false,
      },
    },
    {
      command: ['projects', 'create', 'Cloud'],
      path: `/organizations/${ORGANIZATION_ID}/projects`,
      body: { name: 'Cloud' },
      response: {
        organizationId: ORGANIZATION_ID,
        id: PROJECT_ID,
        name: 'Cloud',
        aggregateVersion: 1,
        createdAt: '2026-07-27T00:00:00.000Z',
        replayed: false,
      },
    },
    {
      command: ['environments', 'create', 'Production'],
      path: `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments`,
      body: { name: 'Production' },
      response: {
        organizationId: ORGANIZATION_ID,
        projectId: PROJECT_ID,
        id: ENVIRONMENT_ID,
        name: 'Production',
        aggregateVersion: 1,
        createdAt: '2026-07-27T00:00:00.000Z',
        replayed: false,
      },
    },
  ] as const)('creates one core tenant resource idempotently %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(testCase.response, 201);
    };
    const output = capture();

    const exitCode = await runCli(
      [...testCase.command, '--idempotency-key=cli:resource-1', '--output=json'],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:resource-1' }),
        body: JSON.stringify(testCase.body),
      })
    );
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it.each([
    ['ready', 'ready'],
    ['drain', 'draining'],
    ['revoke', 'revoked'],
  ] as const)('changes one node lifecycle state idempotently with nodes %s', async (action, state) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(nodeResponse(state));
    };
    const output = capture();

    const exitCode = await runCli(
      ['nodes', action, NODE_ID, '--expected-version=7', '--idempotency-key=cli:node-1', '--output=json'],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/nodes/${NODE_ID}/actions/${action}`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:node-1' }),
        body: JSON.stringify({ expectedVersion: 7 }),
      })
    );
    expect(output.stdout()).toContain(`"state": "${state}"`);
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it.each([
    [
      ['workloads', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workloads`,
    ],
    [['workloads', 'get', WORKLOAD_ID], `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}`],
    [['deployments', 'get', DEPLOYMENT_ID], `/organizations/${ORGANIZATION_ID}/deployments/${DEPLOYMENT_ID}`],
    [
      ['routes', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/routes`,
    ],
    [['routes', 'get', ROUTE_ID], `/organizations/${ORGANIZATION_ID}/routes/${ROUTE_ID}`],
    [
      ['build-runs', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/build-runs?limit=100`,
    ],
    [['build-runs', 'get', BUILD_RUN_ID], `/organizations/${ORGANIZATION_ID}/build-runs/${BUILD_RUN_ID}`],
    [
      ['build-runs', 'evidence', BUILD_RUN_ID],
      `/organizations/${ORGANIZATION_ID}/build-runs/${BUILD_RUN_ID}/evidence`,
    ],
  ] as const)('queries an operational resource through the typed client %#', async (command, path) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(
        command[1] === 'list' ? [] : operationalResource(command[1] === 'evidence' ? 'evidence' : command[0])
      );
    };
    const output = capture();

    const exitCode = await runCli([...command, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stderr()).toBe('');
  });

  it.each([
    [
      ['domain-claims', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/domain-claims`,
      [],
    ],
    [
      ['domain-claims', 'get', DOMAIN_CLAIM_ID],
      `/organizations/${ORGANIZATION_ID}/domain-claims/${DOMAIN_CLAIM_ID}`,
      edgeResource('domain-claim'),
    ],
    [
      ['gateway-scopes', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/gateway-scopes`,
      [],
    ],
  ] as const)('queries an Edge resource through the typed client %#', async (command, path, response) => {
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

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      command: ['domain-claims', 'create', '*.example.test'],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/domain-claims`,
      body: { pattern: '*.example.test' },
      response: { ...edgeResource('domain-claim'), replayed: false },
    },
    {
      command: ['domain-claims', 'verify', DOMAIN_CLAIM_ID, 'a3s-cloud-verification=proof'],
      path: `/organizations/${ORGANIZATION_ID}/domain-claims/${DOMAIN_CLAIM_ID}/verify`,
      body: { proof: 'a3s-cloud-verification=proof' },
      response: { ...edgeResource('domain-claim'), replayed: false },
    },
    {
      command: ['domain-claims', 'revoke', DOMAIN_CLAIM_ID, 'customer request'],
      path: `/organizations/${ORGANIZATION_ID}/domain-claims/${DOMAIN_CLAIM_ID}/revoke`,
      body: { reason: 'customer request' },
      response: { ...edgeResource('domain-claim'), state: 'revoked', replayed: false },
    },
    {
      command: ['gateway-scopes', 'create', NODE_ID, GATEWAY_NODE_ID, '--min-ready=1', '--max-unavailable=1'],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/gateway-scopes`,
      body: { nodeIds: [NODE_ID, GATEWAY_NODE_ID], minReady: 1, maxUnavailable: 1 },
      response: { ...edgeResource('gateway-scope'), replayed: false },
    },
    {
      command: [
        'routes',
        'publish',
        GATEWAY_SCOPE_ID,
        REVISION_ID,
        DOMAIN_CLAIM_ID,
        'api.example.test',
        '/v1',
        'http',
      ],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` + `/environments/${ENVIRONMENT_ID}/routes`,
      body: {
        gatewayScopeId: GATEWAY_SCOPE_ID,
        workloadRevisionId: REVISION_ID,
        domainClaimId: DOMAIN_CLAIM_ID,
        hostname: 'api.example.test',
        pathPrefix: '/v1',
        portName: 'http',
      },
      response: {
        route: operationalResource('routes'),
        certificate: { id: ROUTE_ID, state: 'provisioning' },
        replayed: false,
        commandReplayed: false,
      },
    },
  ] as const)('executes an idempotent Edge mutation %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(testCase.response, 201);
    };
    const output = capture();

    const exitCode = await runCli([...testCase.command, '--idempotency-key=cli:edge-1', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:edge-1' }),
        body: JSON.stringify(testCase.body),
      })
    );
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it('rejects unsafe Gateway rollout input before the network', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const duplicate = capture();
    const threshold = capture();
    const misplaced = capture();

    expect(
      await runCli(['gateway-scopes', 'create', NODE_ID, NODE_ID, '--idempotency-key=cli:scope-duplicate'], {
        ...duplicate.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(
      await runCli(
        ['gateway-scopes', 'create', NODE_ID, '--min-ready=2', '--idempotency-key=cli:scope-threshold'],
        { ...threshold.runtime, environment: completeEnvironment(), fetch: fetcher }
      )
    ).toBe(2);
    expect(
      await runCli(['routes', 'list', '--min-ready=1'], {
        ...misplaced.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);

    expect(called).toBe(false);
    expect(duplicate.stderr()).toContain('must be unique');
    expect(threshold.stderr()).toContain('no greater than the member count');
    expect(misplaced.stderr()).toContain('valid only for gateway-scopes create');
  });

  it.each([
    [
      ['source-revisions', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/source-revisions`,
      [],
    ],
    [
      ['source-connections', 'get'],
      `/organizations/${ORGANIZATION_ID}/source-connections/github`,
      sourceResource('connection'),
    ],
    [
      ['source-subscriptions', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/source-subscriptions/github`,
      [],
    ],
  ] as const)('queries a Source resource through the typed client %#', async (command, path, response) => {
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

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(output.stderr()).toBe('');
  });

  it('starts the no-store GitHub connection flow without inventing an idempotency contract', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(sourceResource('install'), 201);
    };
    const output = capture();

    const exitCode = await runCli(['source-connections', 'begin', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/source-connections/github`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.not.objectContaining({ 'Idempotency-Key': expect.anything() }),
      })
    );
    expect(output.stdout()).toContain('"installationUrl"');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      command: [
        'source-revisions',
        'resolve',
        'https://github.com/A3S-Lab/Cloud.git',
        'branch',
        'main',
        '--context-path=services/api',
        '--dockerfile-path=Dockerfile',
        '--target=release',
        '--platforms=linux/amd64,linux/arm64',
      ],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/source-revisions`,
      body: {
        repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
        reference: { kind: 'branch', value: 'main' },
        recipe: sourceRecipe(),
      },
      response: { ...sourceResource('revision'), replayed: false },
    },
    {
      command: [
        'source-subscriptions',
        'create',
        'https://github.com/A3S-Lab/Cloud.git',
        'main',
        '--context-path=services/api',
        '--dockerfile-path=Dockerfile',
        '--target=release',
        '--platforms=linux/amd64,linux/arm64',
      ],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/source-subscriptions/github`,
      body: {
        repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
        branch: 'main',
        recipe: sourceRecipe(),
      },
      response: { ...sourceResource('subscription'), replayed: false },
    },
    {
      command: ['source-subscriptions', 'deactivate', SOURCE_SUBSCRIPTION_ID],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/source-subscriptions/github/${SOURCE_SUBSCRIPTION_ID}/deactivate`,
      body: undefined,
      response: { ...sourceResource('subscription'), status: 'inactive', replayed: false },
    },
  ] as const)('executes an idempotent Source mutation %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(testCase.response, 201);
    };
    const output = capture();

    const exitCode = await runCli([...testCase.command, '--idempotency-key=cli:source-1', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:source-1' }),
        body: testCase.body === undefined ? undefined : JSON.stringify(testCase.body),
      })
    );
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it('rejects unsafe or misplaced Source inputs before the network', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const cases = [
      [
        'source-revisions',
        'resolve',
        'http://github.com/A3S-Lab/Cloud',
        'branch',
        'main',
        '--context-path=.',
        '--dockerfile-path=Dockerfile',
        '--platforms=linux/amd64',
        '--idempotency-key=cli:source-http',
      ],
      [
        'source-revisions',
        'resolve',
        'https://github.com/A3S--Lab/Cloud',
        'branch',
        'main',
        '--context-path=.',
        '--dockerfile-path=Dockerfile',
        '--platforms=linux/amd64',
        '--idempotency-key=cli:source-owner',
      ],
      [
        'source-revisions',
        'resolve',
        'https://github.com/A3S-Lab/Cloud',
        'pull_request',
        '1',
        '--context-path=.',
        '--dockerfile-path=Dockerfile',
        '--platforms=linux/amd64',
        '--idempotency-key=cli:source-reference',
      ],
      [
        'source-subscriptions',
        'create',
        'https://github.com/A3S-Lab/Cloud',
        'main',
        '--context-path=../escape',
        '--dockerfile-path=Dockerfile',
        '--platforms=linux/amd64',
        '--idempotency-key=cli:source-path',
      ],
      [
        'source-subscriptions',
        'create',
        'https://github.com/A3S-Lab/Cloud',
        'main',
        '--context-path=.',
        '--dockerfile-path=Dockerfile',
        '--platforms=linux/amd64,linux/amd64',
        '--idempotency-key=cli:source-platform',
      ],
      ['organizations', 'list', '--platforms=linux/amd64'],
    ];

    for (const command of cases) {
      const output = capture();
      expect(
        await runCli(command, {
          ...output.runtime,
          environment: completeEnvironment(),
          fetch: fetcher,
        })
      ).toBe(2);
    }
    expect(called).toBe(false);
  });

  it('reads workload logs with bounded query options and exposes the next cursor', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(logPage());
    };
    const output = capture();

    const exitCode = await runCli(
      ['workloads', 'logs', WORKLOAD_ID, REVISION_ID, '--cursor=v1:8', '--limit=25', '--stream=stderr'],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}` +
        `/revisions/${REVISION_ID}/logs?cursor=v1%3A8&limit=25&stream=stderr`
    );
    expect(output.stdout()).toContain('worker ready');
    expect(output.stdout()).toContain('Next cursor: v1:9');
  });

  it('reports that authoritative Box BuildRun logs are unavailable', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return new Response(
        JSON.stringify({
          code: 503,
          statusCode: 'SERVICE_UNAVAILABLE',
          message: 'durable Box build logs are unavailable until Box exposes its build log contract',
          details: {},
          requestId: '019c0000-0000-7000-8000-000000000010',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 503 }
      );
    };
    const output = capture();

    const exitCode = await runCli(['build-runs', 'logs', BUILD_RUN_ID, '--limit=10', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(ExitCode.Api);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/build-runs/${BUILD_RUN_ID}/logs?limit=10`
    );
    expect(output.stdout()).toBe('');
    expect(output.stderr()).toContain('"statusCode": "SERVICE_UNAVAILABLE"');
    expect(output.stderr()).toContain('durable Box build logs are unavailable');
  });

  it.each([
    {
      command: ['workloads', 'stop', WORKLOAD_ID],
      method: 'POST',
      path: `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}/stop`,
      body: undefined,
      response: {
        organizationId: ORGANIZATION_ID,
        workloadId: WORKLOAD_ID,
        operationId: WORKLOAD_ID,
        desiredState: 'stopped',
        requestedAt: '2026-07-26T00:00:00.000Z',
        replayed: false,
      },
    },
    {
      command: ['workloads', 'rollback', WORKLOAD_ID, REVISION_ID],
      method: 'POST',
      path: `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}/rollback`,
      body: JSON.stringify({ revisionId: REVISION_ID }),
      response: {
        organizationId: ORGANIZATION_ID,
        projectId: PROJECT_ID,
        environmentId: ENVIRONMENT_ID,
        workloadId: WORKLOAD_ID,
        revisionId: REVISION_ID,
        deploymentId: DEPLOYMENT_ID,
        operationId: DEPLOYMENT_ID,
        generation: 2,
        status: 'queued',
        artifactSourceUri: 'oci://registry.example.test/api@sha256:abc',
        expectedArtifactDigest: null,
        requestDigest: 'sha256:request',
        artifactDigest: 'sha256:artifact',
        templateDigest: 'sha256:template',
        requestedAt: '2026-07-26T00:00:00.000Z',
        replayed: false,
        skillBindings: [],
      },
    },
    {
      command: ['skill-bindings', 'bind', WORKLOAD_ID, ASSET_ID, ASSET_RELEASE_ID],
      method: 'POST',
      path:
        `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}` +
        `/skills/${ASSET_ID}/releases/${ASSET_RELEASE_ID}/bindings`,
      body: undefined,
      response: workloadDeploymentResponse(),
    },
    {
      command: ['skill-bindings', 'unbind', WORKLOAD_ID, ASSET_ID],
      method: 'DELETE',
      path: `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}` + `/skills/${ASSET_ID}/bindings`,
      body: undefined,
      response: workloadDeploymentResponse(),
    },
    {
      command: ['deployments', 'cancel', DEPLOYMENT_ID],
      method: 'DELETE',
      path: `/organizations/${ORGANIZATION_ID}/deployments/${DEPLOYMENT_ID}`,
      body: undefined,
      response: {
        deploymentId: DEPLOYMENT_ID,
        operationId: DEPLOYMENT_ID,
        status: 'cancelling',
        replayed: false,
      },
    },
    {
      command: ['build-runs', 'cancel', BUILD_RUN_ID],
      method: 'DELETE',
      path: `/organizations/${ORGANIZATION_ID}/build-runs/${BUILD_RUN_ID}`,
      body: undefined,
      response: {
        buildRunId: BUILD_RUN_ID,
        operationId: BUILD_RUN_ID,
        status: 'cancelling',
        cancellationRequestedAt: '2026-07-26T00:00:00.000Z',
        replayed: false,
      },
    },
    {
      command: ['build-runs', 'retry', BUILD_RUN_ID],
      method: 'POST',
      path: `/organizations/${ORGANIZATION_ID}/build-runs/${BUILD_RUN_ID}/retry`,
      body: undefined,
      response: {
        buildRunId: '019c0000-0000-7000-8000-000000000009',
        operationId: '019c0000-0000-7000-8000-000000000009',
        sourceRevisionId: REVISION_ID,
        attempt: 2,
        retryOfBuildRunId: BUILD_RUN_ID,
        status: 'queued',
        replayed: false,
      },
    },
  ] as const)('executes an explicitly idempotent operational mutation %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(testCase.response, 202);
    };
    const output = capture();

    const exitCode = await runCli(
      [...testCase.command, '--idempotency-key=cli:mutation-1', '--output=json'],
      { ...output.runtime, environment: completeEnvironment(), fetch: fetcher }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: testCase.method,
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:mutation-1' }),
        body: testCase.body,
      })
    );
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it.each([
    {
      command: ['workloads', 'create'],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/workloads`,
    },
    {
      command: ['workloads', 'update', WORKLOAD_ID],
      path: `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}/deployments`,
    },
    {
      command: ['source-revisions', 'deploy', REVISION_ID],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/source-revisions/${REVISION_ID}/workloads`,
    },
    {
      command: ['asset-releases', 'deploy', ASSET_ID, ASSET_RELEASE_ID],
      path:
        `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/assets/${ASSET_ID}` +
        `/releases/${ASSET_RELEASE_ID}/workloads`,
    },
    {
      command: ['asset-releases', 'update', WORKLOAD_ID, ASSET_ID, ASSET_RELEASE_ID],
      path:
        `/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}` +
        `/assets/${ASSET_ID}/releases/${ASSET_RELEASE_ID}/deployments`,
    },
  ] as const)('submits one unchanged ACL desired-state mutation %#', async (testCase) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const readPaths: string[] = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope(workloadDeploymentResponse(), 202);
    };
    const manifest = 'version = 1\nworkload "api" {}\n';
    const output = capture();

    const exitCode = await runCli(
      [...testCase.command, '--file=deploy/workload.acl', '--idempotency-key=cli:acl-1', '--output=json'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readFile: async (path) => {
          readPaths.push(path);
          return new TextEncoder().encode(manifest);
        },
      }
    );

    expect(exitCode).toBe(0);
    expect(readPaths).toEqual(['deploy/workload.acl']);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${testCase.path}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'cli:acl-1',
        }),
        body: manifest,
      })
    );
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it('rejects missing, unreadable, invalid, oversized, and misplaced ACL files before the network', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const missing = capture();
    const unreadable = capture();
    const invalidUtf8 = capture();
    const oversized = capture();
    const readOnly = capture();

    expect(
      await runCli(['workloads', 'create', '--idempotency-key=cli:acl-missing'], {
        ...missing.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(
      await runCli(['workloads', 'create', '--file=missing.acl', '--idempotency-key=cli:acl-read'], {
        ...unreadable.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readFile: async () => {
          throw new Error('private filesystem detail');
        },
      })
    ).toBe(2);
    expect(
      await runCli(['workloads', 'update', WORKLOAD_ID, '--file=bad.acl', '--idempotency-key=cli:acl-utf8'], {
        ...invalidUtf8.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readFile: async () => Uint8Array.from([0xff]),
      })
    ).toBe(2);
    expect(
      await runCli(['workloads', 'create', '--file=large.acl', '--idempotency-key=cli:acl-large'], {
        ...oversized.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
        readFile: async () => new Uint8Array(65_537),
      })
    ).toBe(2);
    expect(
      await runCli(['routes', 'get', ROUTE_ID, '--file=read-only.acl'], {
        ...readOnly.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);

    expect(called).toBe(false);
    expect(missing.stderr()).toContain('--file is required');
    expect(unreadable.stderr()).toContain('unable to read');
    expect(unreadable.stderr()).not.toContain('private filesystem detail');
    expect(invalidUtf8.stderr()).toContain('valid UTF-8');
    expect(oversized.stderr()).toContain('between 1 and 65536');
    expect(readOnly.stderr()).toContain('valid only for ACL desired-state mutations');
  });

  it('rejects missing, unsafe, and read-only idempotency options before the network', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const missing = capture();
    const unsafe = capture();
    const readOnly = capture();

    expect(
      await runCli(['workloads', 'stop', WORKLOAD_ID], {
        ...missing.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(
      await runCli(['build-runs', 'retry', BUILD_RUN_ID, '--idempotency-key=contains space'], {
        ...unsafe.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(
      await runCli(['routes', 'get', ROUTE_ID, '--idempotency-key=read-only'], {
        ...readOnly.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);

    expect(called).toBe(false);
    expect(missing.stderr()).toContain('--idempotency-key is required');
    expect(unsafe.stderr()).toContain('idempotency key is invalid');
    expect(readOnly.stderr()).toContain('valid only for mutation commands');
  });

  it('rejects invalid resource IDs and log options before the network', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const invalidId = capture();
    const invalidLimit = capture();

    expect(
      await runCli(['routes', 'get', 'not-a-uuid'], {
        ...invalidId.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(
      await runCli(['build-runs', 'logs', BUILD_RUN_ID, '--limit=257'], {
        ...invalidLimit.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(called).toBe(false);
    expect(invalidId.stderr()).toContain('route ID must be a UUID');
    expect(invalidLimit.stderr()).toContain('log limit must be between 1 and 256');
  });

  it('rejects missing, invalid, and misplaced node versions before the network', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const missing = capture();
    const invalid = capture();
    const misplaced = capture();

    expect(
      await runCli(['nodes', 'drain', NODE_ID, '--idempotency-key=cli:node-missing'], {
        ...missing.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);
    expect(
      await runCli(
        ['nodes', 'ready', NODE_ID, '--expected-version=0', '--idempotency-key=cli:node-invalid'],
        { ...invalid.runtime, environment: completeEnvironment(), fetch: fetcher }
      )
    ).toBe(2);
    expect(
      await runCli(['organizations', 'list', '--expected-version=1'], {
        ...misplaced.runtime,
        environment: completeEnvironment(),
        fetch: fetcher,
      })
    ).toBe(2);

    expect(called).toBe(false);
    expect(missing.stderr()).toContain('--expected-version is required');
    expect(invalid.stderr()).toContain('positive safe integer');
    expect(misplaced.stderr()).toContain('valid only for node lifecycle mutations');
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

  it('reports an unsupported command without requiring an API token', async () => {
    const output = capture();

    const exitCode = await runCli(['unknown', 'list'], output.runtime);

    expect(exitCode).toBe(2);
    expect(output.stderr()).toContain('unsupported command');
    expect(output.stderr()).not.toContain('A3S_CLOUD_TOKEN is required');
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

function completeEnvironment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
    A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
  };
}

function operationalResource(kind: string): Record<string, unknown> {
  if (kind === 'workloads') {
    return { id: WORKLOAD_ID, name: 'worker', desiredState: 'running', deployments: [] };
  }
  if (kind === 'deployments') {
    return { id: DEPLOYMENT_ID, workloadId: WORKLOAD_ID, status: 'active', revision: { generation: 1 } };
  }
  if (kind === 'routes') {
    return { id: ROUTE_ID, hostname: 'worker.example.test', pathPrefix: '/', state: 'active' };
  }
  if (kind === 'build-runs') {
    return { id: BUILD_RUN_ID, status: 'succeeded', attempt: 1, sourceRevisionId: REVISION_ID };
  }
  return {
    schema: 'a3s.build-evidence.v1',
    buildRunId: BUILD_RUN_ID,
    repository: 'A3S-Lab/Cloud',
    commitSha: 'a'.repeat(40),
    verificationState: 'verified',
    artifact: { digest: `sha256:${'b'.repeat(64)}` },
  };
}

function edgeResource(kind: 'domain-claim' | 'gateway-scope'): Record<string, unknown> {
  if (kind === 'gateway-scope') {
    return {
      id: GATEWAY_SCOPE_ID,
      organizationId: ORGANIZATION_ID,
      projectId: PROJECT_ID,
      environmentId: ENVIRONMENT_ID,
      nodeId: NODE_ID,
      memberNodeIds: [NODE_ID, GATEWAY_NODE_ID],
      membershipGeneration: 1,
      minReady: 1,
      maxUnavailable: 1,
      aggregateVersion: 1,
      createdAt: '2026-07-27T00:00:00.000Z',
      updatedAt: '2026-07-27T00:00:00.000Z',
    };
  }
  return {
    id: DOMAIN_CLAIM_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    pattern: '*.example.test',
    challengeDnsName: '_a3s-cloud-challenge.example.test',
    challengeValue: 'a3s-cloud-verification=proof',
    state: 'pending',
    failure: null,
    aggregateVersion: 1,
    createdAt: '2026-07-27T00:00:00.000Z',
    updatedAt: '2026-07-27T00:00:00.000Z',
    verifiedAt: null,
    revokedAt: null,
  };
}

function sourceRecipe(): Record<string, unknown> {
  return {
    schema: 'a3s.cloud.build-recipe.v1',
    kind: 'dockerfile',
    contextPath: 'services/api',
    dockerfilePath: 'Dockerfile',
    target: 'release',
    platforms: ['linux/amd64', 'linux/arm64'],
  };
}

function sourceResource(
  kind: 'connection' | 'install' | 'revision' | 'subscription'
): Record<string, unknown> {
  if (kind === 'install') {
    return {
      provider: 'github',
      installationUrl: 'https://github.com/apps/a3s-cloud/installations/new?state=opaque',
      expiresAt: '2026-07-27T00:10:00.000Z',
    };
  }
  if (kind === 'connection') {
    return {
      id: SOURCE_SUBSCRIPTION_ID,
      organizationId: ORGANIZATION_ID,
      provider: 'github',
      installationId: 42,
      account: { id: 7, login: 'A3S-Lab', type: 'organization' },
      verifiedBy: { id: 8, login: 'operator' },
      status: 'active',
      providerAuthority: {
        checkedAt: '2026-07-27T00:00:00.000Z',
        checkAttemptedAt: '2026-07-27T00:00:00.000Z',
        nextCheckAt: '2026-07-27T00:05:00.000Z',
        consecutiveFailures: 0,
        lastError: null,
      },
      connectedAt: '2026-07-27T00:00:00.000Z',
      updatedAt: '2026-07-27T00:00:00.000Z',
    };
  }
  if (kind === 'subscription') {
    return {
      id: SOURCE_SUBSCRIPTION_ID,
      organizationId: ORGANIZATION_ID,
      projectId: PROJECT_ID,
      environmentId: ENVIRONMENT_ID,
      connectionId: SOURCE_SUBSCRIPTION_ID,
      installationId: 42,
      repository: {
        provider: 'github',
        canonicalUrl: 'https://github.com/a3s-lab/cloud',
        identity: 'github:github.com/a3s-lab/cloud',
      },
      branch: 'main',
      recipe: sourceRecipe(),
      recipeDigest: 'sha256:recipe',
      status: 'active',
      aggregateVersion: 1,
      createdAt: '2026-07-27T00:00:00.000Z',
      deactivatedAt: null,
    };
  }
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    id: SOURCE_REVISION_ID,
    repository: {
      provider: 'github',
      canonicalUrl: 'https://github.com/a3s-lab/cloud',
      identity: 'github:github.com/a3s-lab/cloud',
    },
    commitSha: 'a'.repeat(40),
    recipe: sourceRecipe(),
    recipeDigest: 'sha256:recipe',
    aggregateVersion: 1,
    acceptedAt: '2026-07-27T00:00:00.000Z',
  };
}

function logPage(): Record<string, unknown> {
  return {
    workloadId: WORKLOAD_ID,
    revisionId: REVISION_ID,
    nodeId: ORGANIZATION_ID,
    unitId: 'runtime-unit',
    generation: 1,
    records: [
      {
        kind: 'data',
        sourceCursor: 'provider:9',
        sequence: 9,
        observedAtMs: 1_774_723_200_000,
        stream: 'stderr',
        data: 'worker ready',
        gapReason: null,
        fromSequence: null,
        throughSequence: null,
        compactedChunks: null,
      },
    ],
    nextCursor: 'v1:9',
  };
}

function workloadDeploymentResponse(): Record<string, unknown> {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    workloadId: WORKLOAD_ID,
    revisionId: REVISION_ID,
    deploymentId: DEPLOYMENT_ID,
    operationId: DEPLOYMENT_ID,
    generation: 1,
    status: 'queued',
    artifactSourceUri: 'oci://registry.example.test/api@sha256:abc',
    expectedArtifactDigest: null,
    requestDigest: 'sha256:request',
    artifactDigest: 'sha256:artifact',
    templateDigest: 'sha256:template',
    requestedAt: '2026-07-27T00:00:00.000Z',
    replayed: false,
    skillBindings: [],
  };
}

function nodeResponse(state: 'ready' | 'draining' | 'revoked'): Record<string, unknown> {
  return {
    id: NODE_ID,
    organizationId: ORGANIZATION_ID,
    name: 'worker-1',
    state,
    availability: 'online',
    agentInstanceId: '019c0000-0000-7000-8000-000000000010',
    agentVersion: '0.1.0',
    runtimeProviderId: 'a3s-box',
    runtimeProviderBuild: '3.2.0',
    capabilitiesDigest: 'sha256:capabilities',
    capabilities: {},
    enrolledAt: '2026-07-27T00:00:00.000Z',
    lastObservedAt: '2026-07-27T00:00:00.000Z',
    aggregateVersion: 8,
    replayed: false,
  };
}
