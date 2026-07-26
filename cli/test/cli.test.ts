import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const WORKLOAD_ID = '019c0000-0000-7000-8000-000000000004';
const REVISION_ID = '019c0000-0000-7000-8000-000000000005';
const ROUTE_ID = '019c0000-0000-7000-8000-000000000006';
const DEPLOYMENT_ID = '019c0000-0000-7000-8000-000000000007';
const BUILD_RUN_ID = '019c0000-0000-7000-8000-000000000008';

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

  it('reads BuildRun logs as stable JSON', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return envelope({ ...logPage(), buildRunId: BUILD_RUN_ID, operationId: BUILD_RUN_ID });
    };
    const output = capture();

    const exitCode = await runCli(['build-runs', 'logs', BUILD_RUN_ID, '--limit=10', '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: fetcher,
    });

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/build-runs/${BUILD_RUN_ID}/logs?limit=10`
    );
    expect(output.stdout()).toContain(`"buildRunId": "${BUILD_RUN_ID}"`);
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
      },
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
