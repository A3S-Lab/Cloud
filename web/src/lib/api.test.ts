import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ServiceTemplate, SourceWorkloadTemplate } from '../types/api';
import { CloudApi, type CloudApiError } from './api';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('CloudApi', () => {
  it('sends the token only in the authorization header', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({ code: 200, message: 'Success', data: [], requestId: '1', timestamp: 'now' }),
        {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }
      )
    );
    vi.stubGlobal('fetch', fetchMock);
    const api = new CloudApi('a3s_secret');

    await api.listOrganizations();

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/organizations',
      expect.objectContaining({
        headers: expect.objectContaining({ Authorization: 'Bearer a3s_secret' }),
      })
    );
    expect(fetchMock.mock.calls[0][0]).not.toContain('a3s_secret');
  });

  it('preserves safe API error metadata', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            code: 401,
            statusCode: 'UNAUTHORIZED',
            message: 'missing authentication credentials',
            details: {},
            requestId: 'request-1',
            timestamp: 'now',
          }),
          { status: 401, headers: { 'content-type': 'application/json' } }
        )
      )
    );

    await expect(new CloudApi('bad').listOrganizations()).rejects.toEqual(
      expect.objectContaining<Partial<CloudApiError>>({
        status: 401,
        statusCode: 'UNAUTHORIZED',
        requestId: 'request-1',
      })
    );
  });

  it('scopes workload reads to the selected environment without leaking the token', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        new Response(
          JSON.stringify({ code: 200, message: 'Success', data: [], requestId: '1', timestamp: 'now' }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      );
    vi.stubGlobal('fetch', fetchMock);

    await new CloudApi('a3s_secret').listWorkloads('org / one', 'project', 'production');

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/organizations/org%20%2F%20one/projects/project/environments/production/workloads',
      expect.objectContaining({
        headers: expect.objectContaining({ Authorization: 'Bearer a3s_secret' }),
      })
    );
    expect(fetchMock.mock.calls[0][0]).not.toContain('a3s_secret');
  });

  it('cancels one deployment with an explicit stable idempotency key', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          code: 202,
          message: 'Success',
          data: {
            deploymentId: 'deployment / one',
            operationId: 'operation-1',
            status: 'cancelling',
            replayed: false,
          },
          requestId: '1',
          timestamp: 'now',
        }),
        { status: 202, headers: { 'content-type': 'application/json' } }
      )
    );
    vi.stubGlobal('fetch', fetchMock);

    const result = await new CloudApi('a3s_secret').cancelDeployment(
      'organization',
      'deployment / one',
      'web-cancel:deployment-1'
    );

    expect(result.status).toBe('cancelling');
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/organizations/organization/deployments/deployment%20%2F%20one',
      expect.objectContaining({
        method: 'DELETE',
        headers: expect.objectContaining({
          Authorization: 'Bearer a3s_secret',
          'Idempotency-Key': 'web-cancel:deployment-1',
        }),
      })
    );
  });

  it('lists and cancels build runs within the selected tenant context', async () => {
    const fetchMock = vi.fn().mockImplementation((input: string | URL | Request) => {
      const path = String(input);
      const data = path.includes('/projects/')
        ? []
        : {
            buildRunId: 'build / one',
            operationId: 'operation-1',
            status: 'cancelling',
            cancellationRequestedAt: '2026-07-22T00:00:00Z',
            replayed: false,
          };
      return Promise.resolve(
        new Response(
          JSON.stringify({ code: path.includes('/projects/') ? 200 : 202, message: 'Success', data }),
          { status: path.includes('/projects/') ? 200 : 202, headers: { 'content-type': 'application/json' } }
        )
      );
    });
    vi.stubGlobal('fetch', fetchMock);
    const api = new CloudApi('a3s_secret');

    await api.listBuildRuns('organization', 'project / one', 'production');
    const cancelled = await api.cancelBuildRun('organization', 'build / one', 'web-cancel-build:build-1');

    expect(cancelled.status).toBe('cancelling');
    expect(fetchMock.mock.calls[0][0]).toBe(
      '/api/v1/organizations/organization/projects/project%20%2F%20one/environments/production/build-runs?limit=100'
    );
    expect(fetchMock.mock.calls[1]).toEqual([
      '/api/v1/organizations/organization/build-runs/build%20%2F%20one',
      expect.objectContaining({
        method: 'DELETE',
        headers: expect.objectContaining({
          Authorization: 'Bearer a3s_secret',
          'Idempotency-Key': 'web-cancel-build:build-1',
        }),
      }),
    ]);
  });

  it('stops one active workload through its own durable operation', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          code: 202,
          message: 'Success',
          data: {
            organizationId: 'organization',
            workloadId: 'workload / one',
            operationId: 'operation-2',
            desiredState: 'stopped',
            requestedAt: 'now',
            replayed: false,
          },
          requestId: '1',
          timestamp: 'now',
        }),
        { status: 202, headers: { 'content-type': 'application/json' } }
      )
    );
    vi.stubGlobal('fetch', fetchMock);

    const result = await new CloudApi('a3s_secret').stopWorkload(
      'organization',
      'workload / one',
      'web-stop:workload-1'
    );

    expect(result.desiredState).toBe('stopped');
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/organizations/organization/workloads/workload%20%2F%20one/stop',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Bearer a3s_secret',
          'Idempotency-Key': 'web-stop:workload-1',
        }),
      })
    );
  });

  it('loads authoritative route and certificate projections for the selected context', async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation(() =>
        Promise.resolve(
          new Response(
            JSON.stringify({ code: 200, message: 'Success', data: [], requestId: '1', timestamp: 'now' }),
            { status: 200, headers: { 'content-type': 'application/json' } }
          )
        )
      );
    vi.stubGlobal('fetch', fetchMock);
    const api = new CloudApi('a3s_secret');

    await api.listRoutes('organization', 'project / one', 'production');
    await api.listGatewayCertificates('organization');

    expect(fetchMock.mock.calls[0][0]).toBe(
      '/api/v1/organizations/organization/projects/project%20%2F%20one/environments/production/routes'
    );
    expect(fetchMock.mock.calls[1][0]).toBe('/api/v1/organizations/organization/gateway-certificates');
  });

  it('submits a complete immutable template and an explicit rollback source', async () => {
    const fetchMock = vi.fn().mockImplementation(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            code: 202,
            message: 'Success',
            data: {
              organizationId: 'organization',
              projectId: 'project',
              environmentId: 'environment',
              workloadId: 'workload',
              revisionId: 'revision',
              deploymentId: 'deployment',
              operationId: 'operation',
              generation: 2,
              status: 'queued',
              artifactSourceUri: 'oci://registry.example/cloud/api:v2',
              expectedArtifactDigest: null,
              requestDigest: 'sha256:request',
              artifactDigest: null,
              templateDigest: null,
              requestedAt: 'now',
              replayed: false,
            },
            requestId: '1',
            timestamp: 'now',
          }),
          { status: 202, headers: { 'content-type': 'application/json' } }
        )
      )
    );
    vi.stubGlobal('fetch', fetchMock);
    const api = new CloudApi('a3s_secret');
    const template: ServiceTemplate = {
      artifact: {
        uri: 'oci://registry.example/cloud/api:v2',
        expectedDigest: null,
      },
      process: {
        command: [],
        args: [],
        workingDirectory: null,
        environment: {},
      },
      secrets: [],
      resources: {
        cpuMillis: 100,
        memoryBytes: 33_554_432,
        pids: 32,
        ephemeralStorageBytes: null,
      },
      ports: [{ name: 'http', containerPort: 8080 }],
      health: {
        portName: 'http',
        path: '/health',
        intervalMs: 1_000,
        timeoutMs: 500,
        healthyThreshold: 1,
        unhealthyThreshold: 3,
        stabilizationWindowMs: 1_000,
      },
    };

    await api.updateWorkload('organization', 'workload / one', template, 'web-update:key');
    await api.rollbackWorkload('organization', 'workload / one', 'revision / one', 'web-rollback:key');
    const sourceTemplate: SourceWorkloadTemplate = {
      process: template.process,
      secrets: template.secrets,
      resources: template.resources,
      ports: template.ports,
      health: template.health,
    };
    await api.deploySourceRevision(
      'organization',
      'project / one',
      'production',
      'source / one',
      'api',
      sourceTemplate,
      'web-source-deploy:key'
    );

    expect(fetchMock.mock.calls[0][0]).toBe(
      '/api/v1/organizations/organization/workloads/workload%20%2F%20one/deployments'
    );
    expect(fetchMock.mock.calls[0][1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'web-update:key',
        }),
        body: JSON.stringify({ template }),
      })
    );
    expect(fetchMock.mock.calls[1][0]).toBe(
      '/api/v1/organizations/organization/workloads/workload%20%2F%20one/rollback'
    );
    expect(fetchMock.mock.calls[1][1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({ revisionId: 'revision / one' }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'web-rollback:key' }),
      })
    );
    expect(fetchMock.mock.calls[2][0]).toBe(
      '/api/v1/organizations/organization/projects/project%20%2F%20one/environments/production/source-revisions/source%20%2F%20one/workloads'
    );
    expect(fetchMock.mock.calls[2][1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({ name: 'api', template: sourceTemplate }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'web-source-deploy:key' }),
      })
    );
  });

  it('builds a scoped live-log URL without putting the token in it', () => {
    const api = new CloudApi('a3s_secret');

    const url = api.workloadLogStreamUrl('organization / one', 'workload / one', 'revision / one', 'stderr');

    expect(url).toBe(
      '/api/v1/organizations/organization%20%2F%20one/workloads/workload%20%2F%20one/revisions/revision%20%2F%20one/logs/stream?limit=16&stream=stderr'
    );
    expect(url).not.toContain('a3s_secret');
  });

  it('builds a tenant-scoped build-log URL without putting the token in it', () => {
    const api = new CloudApi('a3s_secret');

    const url = api.buildRunLogStreamUrl('organization / one', 'build / one', 'stdout');

    expect(url).toBe(
      '/api/v1/organizations/organization%20%2F%20one/build-runs/build%20%2F%20one/logs/stream?limit=16&stream=stdout'
    );
    expect(url).not.toContain('a3s_secret');
  });
});
