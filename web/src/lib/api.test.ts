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
});
