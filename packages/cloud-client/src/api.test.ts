import { describe, expect, it } from 'bun:test';
import { CloudApi, CloudApiError, type CloudFetch } from './api';

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

  it('exposes the tenant-scoped node projection', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse([]);
    };

    await new CloudApi('token', '/api/v1', { fetch: fetcher }).listNodes('organization');

    expect(calls[0]?.[0]).toBe('/api/v1/organizations/organization/nodes');
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
