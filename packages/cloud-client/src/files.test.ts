import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';
import {
  DEFAULT_USER_FILE_LIST_LIMIT,
  encodeUserFileListOptions,
  MAXIMUM_USER_FILE_LIST_LIMIT,
  USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
  validateExpectedUserFileVersion,
  validateUserFileAdmissionAcl,
} from './files';

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-08-28T00:00:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
  );
}

describe('UserFile client surface', () => {
  it('uses the exact metadata lifecycle routes and never adds a buffered binary upload route', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const admissionAcl = 'user_file {\n  schema = "cloud.user-file.v1"\n}\n';

    await api.reserveUserFile(
      'organization / one',
      'project / one',
      { admissionAcl, privateBytes: 'must-not-cross-boundary' } as { admissionAcl: string },
      'files:reserve'
    );
    await api.listUserFiles('organization / one', 'project / one');
    await api.getUserFile('organization / one', 'project / one', 'file / one');
    await api.tombstoneUserFile('organization / one', 'project / one', 'file / one', 1, 'files:tombstone');
    await api.getUserFileQuota('organization / one');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/user-files',
      `/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/user-files?limit=${DEFAULT_USER_FILE_LIST_LIMIT}`,
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/user-files/file%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/user-files/file%20%2F%20one/tombstone',
      '/api/v1/organizations/organization%20%2F%20one/user-file-quota',
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'files:reserve' }),
        body: JSON.stringify({ admissionAcl }),
      })
    );
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'files:tombstone' }),
        body: JSON.stringify({ expectedVersion: 1 }),
      })
    );
    expect(JSON.stringify(calls)).not.toContain('must-not-cross-boundary');
    expect(calls.map(([input]) => String(input)).some((path) => path.endsWith('/upload'))).toBe(false);
  });

  it('enforces ACL transport bounds, list bounds, and optimistic version bounds locally', () => {
    expect(encodeUserFileListOptions()).toBe(`?limit=${DEFAULT_USER_FILE_LIST_LIMIT}`);
    expect(encodeUserFileListOptions({ limit: MAXIMUM_USER_FILE_LIST_LIMIT })).toBe(
      `?limit=${MAXIMUM_USER_FILE_LIST_LIMIT}`
    );
    expect(() => encodeUserFileListOptions({ limit: 0 })).toThrow(RangeError);
    expect(() => encodeUserFileListOptions({ limit: MAXIMUM_USER_FILE_LIST_LIMIT + 1 })).toThrow(RangeError);
    expect(() => validateExpectedUserFileVersion(0)).toThrow(RangeError);
    expect(() => validateExpectedUserFileVersion(Number.MAX_SAFE_INTEGER + 1)).toThrow(RangeError);
    expect(() => validateUserFileAdmissionAcl('')).toThrow(TypeError);
    expect(() => validateUserFileAdmissionAcl('user_file {\rinvalid = true\n}\n')).toThrow(TypeError);
    expect(() =>
      validateUserFileAdmissionAcl('界'.repeat(USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES))
    ).toThrow(TypeError);
  });

  it('rejects malformed values before issuing a request', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.reserveUserFile('organization', 'project', { admissionAcl: '' }, 'files:reserve')
    ).toThrow(TypeError);
    expect(() => api.tombstoneUserFile('organization', 'project', 'file', 0, 'files:tombstone')).toThrow(
      RangeError
    );
    expect(() => api.listUserFiles('organization', 'project', { limit: 201 })).toThrow(RangeError);
    expect(called).toBe(false);
  });
});
