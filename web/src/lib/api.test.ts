import { afterEach, describe, expect, it, vi } from 'vitest';
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

  it('builds a scoped live-log URL without putting the token in it', () => {
    const api = new CloudApi('a3s_secret');

    const url = api.workloadLogStreamUrl('organization / one', 'workload / one', 'revision / one', 'stderr');

    expect(url).toBe(
      '/api/v1/organizations/organization%20%2F%20one/workloads/workload%20%2F%20one/revisions/revision%20%2F%20one/logs/stream?limit=16&stream=stderr'
    );
    expect(url).not.toContain('a3s_secret');
  });
});
