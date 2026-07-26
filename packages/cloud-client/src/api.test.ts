import { describe, expect, it } from 'bun:test';
import { A3S_ACL_MEDIA_TYPE, CloudApi, CloudApiError, type CloudFetch, MAX_WORKLOAD_ACL_BYTES } from './api';

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-07-26T00:00:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
  );
}

describe('CloudApi', () => {
  it('uses the shared authenticated transport and encodes tenant paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse([]);
    };
    const api = new CloudApi('a3s_secret', 'https://cloud.example.test/api/v1/', { fetch: fetcher });

    await api.listProjects('organization / one');

    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(
      'https://cloud.example.test/api/v1/organizations/organization%20%2F%20one/projects'
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'GET',
        headers: expect.objectContaining({ Authorization: 'Bearer a3s_secret' }),
      })
    );
    expect(String(calls[0]?.[0])).not.toContain('a3s_secret');
  });

  it('reads public platform and health diagnostics without requiring an authorization header', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      if (path.endsWith('/platform')) {
        return jsonResponse({ name: 'a3s-cloud', version: '0.1.0', role: 'api' });
      }
      if (path.endsWith('/health/live')) {
        return jsonResponse({ status: 'up', checks: {} });
      }
      return jsonResponse(
        {
          status: 'down',
          checks: { repositories: { status: 'down', details: { reason: 'unavailable' } } },
        },
        503
      );
    };

    const diagnostics = await new CloudApi(undefined, '/api/v1', { fetch: fetcher }).getDiagnostics();

    expect(diagnostics).toEqual({
      platform: { name: 'a3s-cloud', version: '0.1.0', role: 'api' },
      liveness: { status: 'up', checks: {} },
      readiness: {
        status: 'down',
        checks: { repositories: { status: 'down', details: { reason: 'unavailable' } } },
      },
    });
    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/platform',
      '/api/v1/health/live',
      '/api/v1/health/ready',
    ]);
    for (const [, init] of calls) {
      expect(init?.headers).not.toHaveProperty('Authorization');
    }
  });

  it('does not reinterpret a real readiness error envelope as a health report', async () => {
    const api = new CloudApi(undefined, '/api/v1', {
      fetch: async () =>
        new Response(
          JSON.stringify({
            code: 503,
            statusCode: 'SERVICE_UNAVAILABLE',
            message: 'Service unavailable',
            details: {},
            requestId: '019c0000-0000-7000-8000-000000000001',
            timestamp: '2026-07-27T00:00:00.000Z',
          }),
          { status: 503, headers: { 'content-type': 'application/json' } }
        ),
    });

    await expect(api.getReadiness()).rejects.toMatchObject({
      status: 503,
      statusCode: 'SERVICE_UNAVAILABLE',
    });
  });

  it('exposes the tenant-scoped node projection', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse([]);
    };

    await new CloudApi('token', '/api/v1', { fetch: fetcher }).listNodes('organization');

    expect(calls[0]?.[0]).toBe('/api/v1/organizations/organization/nodes');
  });

  it('creates core tenant resources through their existing idempotent REST paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 201);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.createOrganization('Operations', 'cli:organization-1');
    await api.createProject('organization', 'Cloud', 'cli:project-1');
    await api.createEnvironment('organization', 'project', 'Production', 'cli:environment-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        contentType: (init?.headers as Record<string, string>)['Content-Type'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations',
        method: 'POST',
        idempotencyKey: 'cli:organization-1',
        contentType: 'application/json',
        body: JSON.stringify({ name: 'Operations' }),
      },
      {
        input: '/api/v1/organizations/organization/projects',
        method: 'POST',
        idempotencyKey: 'cli:project-1',
        contentType: 'application/json',
        body: JSON.stringify({ name: 'Cloud' }),
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments',
        method: 'POST',
        idempotencyKey: 'cli:environment-1',
        contentType: 'application/json',
        body: JSON.stringify({ name: 'Production' }),
      },
    ]);
  });

  it('changes node lifecycle state with explicit optimistic concurrency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false });
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.markNodeReady('organization', 'node', 3, 'cli:node-ready-1');
    await api.drainNode('organization', 'node', 4, 'cli:node-drain-1');
    await api.revokeNode('organization', 'node', 5, 'cli:node-revoke-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/nodes/node/actions/ready',
        method: 'POST',
        idempotencyKey: 'cli:node-ready-1',
        body: JSON.stringify({ expectedVersion: 3 }),
      },
      {
        input: '/api/v1/organizations/organization/nodes/node/actions/drain',
        method: 'POST',
        idempotencyKey: 'cli:node-drain-1',
        body: JSON.stringify({ expectedVersion: 4 }),
      },
      {
        input: '/api/v1/organizations/organization/nodes/node/actions/revoke',
        method: 'POST',
        idempotencyKey: 'cli:node-revoke-1',
        body: JSON.stringify({ expectedVersion: 5 }),
      },
    ]);
  });

  it('rejects an unsafe node aggregate version before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() => api.drainNode('organization', 'node', 0, 'cli:node-drain-1')).toThrow(
      'expected node version must be a positive safe integer'
    );
    expect(() => api.drainNode('organization', 'node', 1.5, 'cli:node-drain-1')).toThrow(
      'expected node version must be a positive safe integer'
    );
    expect(called).toBe(false);
  });

  it('exposes operational resources through the public tenant paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.getWorkload('organization / one', 'workload / one');
    await api.getDeployment('organization / one', 'deployment / one');
    await api.getRoute('organization / one', 'route / one');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/workloads/workload%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/deployments/deployment%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/routes/route%20%2F%20one',
    ]);
  });

  it('exposes complete Edge queries and idempotent mutations through existing REST paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.listDomainClaims('organization', 'project', 'environment');
    await api.getDomainClaim('organization', 'claim');
    await api.createDomainClaim(
      'organization',
      'project',
      'environment',
      '*.example.test',
      'cli:claim-create-1'
    );
    await api.verifyDomainClaim(
      'organization',
      'claim',
      'a3s-cloud-verification=proof',
      'cli:claim-verify-1'
    );
    await api.revokeDomainClaim('organization', 'claim', 'customer request', 'cli:claim-revoke-1');
    await api.listGatewayScopes('organization', 'project', 'environment');
    await api.createGatewayScope(
      'organization',
      'project',
      'environment',
      { nodeIds: ['node-a', 'node-b'], minReady: 1, maxUnavailable: 1 },
      'cli:scope-create-1'
    );
    await api.publishRoute(
      'organization',
      'project',
      'environment',
      {
        gatewayScopeId: 'scope',
        workloadRevisionId: 'revision',
        domainClaimId: 'claim',
        hostname: 'api.example.test',
        pathPrefix: '/v1',
        portName: 'http',
      },
      'cli:route-publish-1'
    );

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/domain-claims',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/domain-claims/claim',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/domain-claims',
        method: 'POST',
        idempotencyKey: 'cli:claim-create-1',
        body: JSON.stringify({ pattern: '*.example.test' }),
      },
      {
        input: '/api/v1/organizations/organization/domain-claims/claim/verify',
        method: 'POST',
        idempotencyKey: 'cli:claim-verify-1',
        body: JSON.stringify({ proof: 'a3s-cloud-verification=proof' }),
      },
      {
        input: '/api/v1/organizations/organization/domain-claims/claim/revoke',
        method: 'POST',
        idempotencyKey: 'cli:claim-revoke-1',
        body: JSON.stringify({ reason: 'customer request' }),
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/gateway-scopes',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/gateway-scopes',
        method: 'POST',
        idempotencyKey: 'cli:scope-create-1',
        body: JSON.stringify({ nodeIds: ['node-a', 'node-b'], minReady: 1, maxUnavailable: 1 }),
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/routes',
        method: 'POST',
        idempotencyKey: 'cli:route-publish-1',
        body: JSON.stringify({
          gatewayScopeId: 'scope',
          workloadRevisionId: 'revision',
          domainClaimId: 'claim',
          hostname: 'api.example.test',
          pathPrefix: '/v1',
          portName: 'http',
        }),
      },
    ]);
  });

  it('exposes Source queries, connection bootstrap, and idempotent mutations through existing REST paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const recipe = {
      schema: 'a3s.cloud.build-recipe.v1' as const,
      kind: 'dockerfile' as const,
      contextPath: 'services/api',
      dockerfilePath: 'Dockerfile',
      target: 'release',
      platforms: ['linux/amd64' as const, 'linux/arm64' as const],
    };

    await api.listSourceRevisions('organization', 'project', 'environment');
    await api.resolveSourceRevision(
      'organization',
      'project',
      'environment',
      {
        repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
        reference: { kind: 'branch', value: 'main' },
        recipe,
      },
      'cli:source-resolve-1'
    );
    await api.getGithubConnection('organization');
    await api.beginGithubConnection('organization');
    await api.listGithubRepositorySubscriptions('organization', 'project', 'environment');
    await api.createGithubRepositorySubscription(
      'organization',
      'project',
      'environment',
      {
        repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
        branch: 'main',
        recipe,
      },
      'cli:source-subscribe-1'
    );
    await api.deactivateGithubRepositorySubscription(
      'organization',
      'project',
      'environment',
      'subscription',
      'cli:source-deactivate-1'
    );

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-revisions',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-revisions',
        method: 'POST',
        idempotencyKey: 'cli:source-resolve-1',
        body: JSON.stringify({
          repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
          reference: { kind: 'branch', value: 'main' },
          recipe,
        }),
      },
      {
        input: '/api/v1/organizations/organization/source-connections/github',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/source-connections/github',
        method: 'POST',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-subscriptions/github',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-subscriptions/github',
        method: 'POST',
        idempotencyKey: 'cli:source-subscribe-1',
        body: JSON.stringify({
          repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
          branch: 'main',
          recipe,
        }),
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-subscriptions/github/subscription/deactivate',
        method: 'POST',
        idempotencyKey: 'cli:source-deactivate-1',
        body: undefined,
      },
    ]);
  });

  it('exposes Secret queries and idempotent mutations without returning plaintext', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const initialValue = 'postgres://cloud:initial@database';
    const rotatedValue = 'postgres://cloud:rotated@database';

    await api.listSecrets('organization', 'project', 'environment');
    await api.getSecret('organization', 'secret');
    await api.createSecret(
      'organization',
      'project',
      'environment',
      'Database URL',
      initialValue,
      'cli:secret-create-1'
    );
    await api.addSecretVersion('organization', 'secret', rotatedValue, 'cli:secret-version-1');
    await api.revokeSecretVersion('organization', 'secret', 1, 'cli:secret-revoke-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/secrets',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/secrets/secret',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/secrets',
        method: 'POST',
        idempotencyKey: 'cli:secret-create-1',
        body: JSON.stringify({ name: 'Database URL', value: initialValue }),
      },
      {
        input: '/api/v1/organizations/organization/secrets/secret/versions',
        method: 'POST',
        idempotencyKey: 'cli:secret-version-1',
        body: JSON.stringify({ value: rotatedValue }),
      },
      {
        input: '/api/v1/organizations/organization/secrets/secret/versions/1/revoke',
        method: 'POST',
        idempotencyKey: 'cli:secret-revoke-1',
        body: undefined,
      },
    ]);
  });

  it('rejects invalid Secret values and versions before transport', async () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createSecret('organization', 'project', 'environment', 'Empty', '', 'cli:secret-empty')
    ).toThrow('Secret value must contain between 1 byte and 1 MiB');
    expect(() =>
      api.addSecretVersion('organization', 'secret', 'é'.repeat(524_289), 'cli:secret-large')
    ).toThrow('Secret value must contain between 1 byte and 1 MiB');
    expect(() => api.revokeSecretVersion('organization', 'secret', 0, 'cli:secret-version-zero')).toThrow(
      'Secret version must be a positive safe integer'
    );
    expect(called).toBe(false);
  });

  it('reads bounded workload and BuildRun log pages with opaque cursors', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ records: [], nextCursor: null });
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.getWorkloadLogs('organization', 'workload', 'revision', {
      cursor: 'v1:41',
      limit: 25,
      stream: 'stderr',
    });
    await api.getBuildRunLogs('organization', 'build-run', {
      cursor: 'v1:9',
      limit: 10,
      stream: 'stdout',
    });

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization/workloads/workload/revisions/revision/logs?cursor=v1%3A41&limit=25&stream=stderr',
      '/api/v1/organizations/organization/build-runs/build-run/logs?cursor=v1%3A9&limit=10&stream=stdout',
    ]);
    expect(() => api.getBuildRunLogs('organization', 'build-run', { limit: 0 })).toThrow(
      'log limit must be between 1 and 256'
    );
    expect(() => api.getBuildRunLogs('organization', 'build-run', { cursor: 'x'.repeat(1_025) })).toThrow(
      'log cursor is invalid'
    );
  });

  it('sends operational mutations with one explicit idempotency key', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.stopWorkload('organization', 'workload', 'cli:stop-1');
    await api.rollbackWorkload('organization', 'workload', 'revision', 'cli:rollback-1');
    await api.cancelDeployment('organization', 'deployment', 'cli:cancel-deployment-1');
    await api.cancelBuildRun('organization', 'build-run', 'cli:cancel-build-1');
    await api.retryBuildRun('organization', 'build-run', 'cli:retry-build-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        headers: init?.headers,
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/workloads/workload/stop',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:stop-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/workloads/workload/rollback',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:rollback-1' }),
        body: JSON.stringify({ revisionId: 'revision' }),
      },
      {
        input: '/api/v1/organizations/organization/deployments/deployment',
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:cancel-deployment-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/build-runs/build-run',
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:cancel-build-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/build-runs/build-run/retry',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:retry-build-1' }),
        body: undefined,
      },
    ]);
  });

  it('sends ACL desired state unchanged through the three workload mutation paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const manifest = 'version = 1\nworkload "api" {}\n';

    await api.createWorkloadFromAcl('organization', 'project', 'environment', manifest, 'cli:create-1');
    await api.updateWorkloadFromAcl('organization', 'workload', manifest, 'cli:update-1');
    await api.deploySourceRevisionFromAcl(
      'organization',
      'project',
      'environment',
      'source-revision',
      manifest,
      'cli:source-1'
    );

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        contentType: (init?.headers as Record<string, string>)['Content-Type'],
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/workloads',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:create-1',
        body: manifest,
      },
      {
        input: '/api/v1/organizations/organization/workloads/workload/deployments',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:update-1',
        body: manifest,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-revisions/source-revision/workloads',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:source-1',
        body: manifest,
      },
    ]);
  });

  it('rejects empty and oversized ACL before transport', async () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createWorkloadFromAcl('organization', 'project', 'environment', '', 'cli:create-1')
    ).toThrow('workload ACL must contain between');
    expect(() =>
      api.updateWorkloadFromAcl(
        'organization',
        'workload',
        'é'.repeat(MAX_WORKLOAD_ACL_BYTES),
        'cli:update-1'
      )
    ).toThrow('workload ACL must contain between');
    expect(called).toBe(false);
  });

  it('rejects unsafe idempotency keys before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    for (const key of ['', 'contains space', 'contains\nnewline', 'é', 'x'.repeat(256)]) {
      await expect(api.stopWorkload('organization', 'workload', key)).rejects.toThrow(
        'idempotency key is invalid'
      );
    }
    expect(called).toBe(false);
  });

  it('creates resumable event-stream headers without putting credentials in URLs', () => {
    const api = new CloudApi('a3s_secret');

    expect(api.eventStreamHeaders('event-7')).toEqual({
      Accept: 'text/event-stream',
      Authorization: 'Bearer a3s_secret',
      'Last-Event-ID': 'event-7',
    });
    expect(api.operationStreamUrl('organization')).not.toContain('a3s_secret');
  });

  it('exposes tenant-scoped API token metadata and idempotent mutations', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const credential = `a3s_${'a'.repeat(64)}`;

    await api.listApiTokens('organization / one');
    await api.getApiToken('organization / one', 'token / one');
    await api.createApiToken(
      'organization / one',
      {
        name: 'automation',
        token: credential,
        scopes: ['project:write', 'build:write'],
        expiresAt: '2027-01-02T03:04:05.000Z',
      },
      'client:token-create'
    );
    await api.revokeApiToken('organization / one', 'token / one', 'client:token-revoke');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/api-tokens',
      '/api/v1/organizations/organization%20%2F%20one/api-tokens/token%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/api-tokens',
      '/api/v1/organizations/organization%20%2F%20one/api-tokens/token%20%2F%20one',
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:token-create' }),
        body: JSON.stringify({
          name: 'automation',
          token: credential,
          scopes: ['project:write', 'build:write'],
          expiresAt: '2027-01-02T03:04:05.000Z',
        }),
      })
    );
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:token-revoke' }),
      })
    );
    expect(calls.every(([input]) => !String(input).includes(credential))).toBe(true);
  });

  it('rejects invalid API token creation input before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    const valid = {
      name: 'automation',
      token: `a3s_${'a'.repeat(64)}`,
      scopes: ['project:write'],
      expiresAt: null,
    };

    expect(() =>
      api.createApiToken('organization', { ...valid, token: 'not-a-token' }, 'client:token-invalid')
    ).toThrow('API token must use the a3s_ prefix followed by 64 lowercase hex digits');
    expect(() => api.createApiToken('organization', { ...valid, scopes: [] }, 'client:token-scopes')).toThrow(
      'API token must grant at least one scope'
    );
    expect(() =>
      api.createApiToken(
        'organization',
        { ...valid, scopes: ['Project:write'] },
        'client:token-invalid-scope'
      )
    ).toThrow('API token scope must use bounded lowercase domain:action syntax');
    expect(() =>
      api.createApiToken('organization', { ...valid, expiresAt: 'tomorrow' }, 'client:token-invalid-expiry')
    ).toThrow('API token expiry must be an RFC 3339 timestamp');
    expect(called).toBe(false);
  });

  it('preserves only the validated API error contract', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 403,
          statusCode: 'FORBIDDEN',
          message: 'insufficient scope',
          details: { requiredScope: 'node:read' },
          requestId: '019c0000-0000-7000-8000-000000000002',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 403 }
      );

    await expect(
      new CloudApi('token', '/api/v1', { fetch: fetcher }).listNodes('organization')
    ).rejects.toEqual(
      expect.objectContaining({
        status: 403,
        statusCode: 'FORBIDDEN',
        requestId: '019c0000-0000-7000-8000-000000000002',
        details: { requiredScope: 'node:read' },
      })
    );
  });

  it('bounds nested error details before exposing them', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 422,
          statusCode: 'UNPROCESSABLE_ENTITY',
          message: 'invalid request',
          details: {
            long: 'x'.repeat(2_000),
            nested: { one: { two: { three: 'hidden' } } },
          },
          requestId: '019c0000-0000-7000-8000-000000000003',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 422 }
      );

    let failure: unknown;
    try {
      await new CloudApi('token', '/api/v1', { fetch: fetcher }).listOrganizations();
    } catch (error) {
      failure = error;
    }

    expect(failure).toBeInstanceOf(CloudApiError);
    const details = (failure as CloudApiError).details;
    expect(String(details.long)).toHaveLength(1_024);
    expect(details.nested).toEqual({ one: { two: { three: '[truncated]' } } });
  });

  it.each([
    ['non-JSON body', new Response('<html>bad gateway</html>', { status: 502 })],
    ['malformed success envelope', new Response(JSON.stringify({ code: 200, data: [] }), { status: 200 })],
    [
      'mismatched status code',
      new Response(
        JSON.stringify({
          code: 500,
          message: 'Success',
          data: [],
          requestId: 'request-1',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 200 }
      ),
    ],
  ])('maps a %s to one sanitized protocol error', async (_name, response) => {
    const fetcher: CloudFetch = async () => response;

    await expect(new CloudApi('token', '/api/v1', { fetch: fetcher }).listOrganizations()).rejects.toEqual(
      expect.objectContaining({
        status: response.status,
        statusCode: 'INVALID_RESPONSE',
        message: 'Cloud API returned an invalid response',
      })
    );
  });

  it('does not expose transport implementation details or credentials', async () => {
    const fetcher: CloudFetch = async () => {
      throw new Error('connection failed while using a3s_secret');
    };

    let failure: unknown;
    try {
      await new CloudApi('a3s_secret', '/api/v1', { fetch: fetcher }).listOrganizations();
    } catch (error) {
      failure = error;
    }

    expect(failure).toEqual(
      expect.objectContaining({
        status: 0,
        statusCode: 'NETWORK_ERROR',
        message: 'Cloud API request failed',
      })
    );
    expect(String(failure)).not.toContain('a3s_secret');
  });

  it('bounds every request with a stable timeout error', async () => {
    const fetcher: CloudFetch = (_input, init) =>
      new Promise((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), {
          once: true,
        });
      });

    await expect(
      new CloudApi('token', '/api/v1', { fetch: fetcher, requestTimeoutMs: 5 }).listOrganizations()
    ).rejects.toEqual(
      expect.objectContaining({
        status: 0,
        statusCode: 'REQUEST_TIMEOUT',
      })
    );
  });

  it('distinguishes caller cancellation from timeout', async () => {
    const fetcher: CloudFetch = (_input, init) =>
      new Promise((_resolve, reject) => {
        if (init?.signal?.aborted) {
          reject(new DOMException('aborted', 'AbortError'));
          return;
        }
        init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), {
          once: true,
        });
      });
    const controller = new AbortController();
    controller.abort();

    await expect(
      new CloudApi('token', '/api/v1', { fetch: fetcher }).listOrganizations(controller.signal)
    ).rejects.toEqual(
      expect.objectContaining({
        statusCode: 'REQUEST_ABORTED',
      })
    );
  });
});
