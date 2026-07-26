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
