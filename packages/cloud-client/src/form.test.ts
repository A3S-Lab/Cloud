import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch, MAX_FORM_DOCUMENT_BYTES } from './api';

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-08-09T00:00:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
  );
}

describe('CloudApi Form lifecycle', () => {
  it('uses the typed draft and immutable release REST lifecycle', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({}, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const draft = {
      name: 'Approval',
      document: {
        kind: 'a3s.form',
        apiVersion: 'a3s.dev/form/v1alpha1',
      },
    };
    const revision = {
      ...draft,
      name: 'Approval request',
      description: 'Manager approval',
    };

    await api.listFormDrafts('organization / one', 'project / one');
    await api.getFormDraft('organization / one', 'form / one');
    await api.createFormDraft('organization / one', 'project / one', draft, 'form:create');
    await api.reviseFormDraft(
      'organization / one',
      'form / one',
      revision,
      { expectedVersion: 1 },
      'form:revise'
    );
    await api.listFormReleases('organization / one', 'form / one');
    await api.getFormRelease('organization / one', 'form / one', 'release / one');
    await api.publishFormRelease('organization / one', 'form / one', { expectedVersion: 2 }, 'form:publish');

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      ['/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/forms', 'GET'],
      ['/api/v1/organizations/organization%20%2F%20one/forms/form%20%2F%20one', 'GET'],
      ['/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/forms', 'POST'],
      ['/api/v1/organizations/organization%20%2F%20one/forms/form%20%2F%20one/draft-revisions', 'POST'],
      ['/api/v1/organizations/organization%20%2F%20one/forms/form%20%2F%20one/releases', 'GET'],
      [
        '/api/v1/organizations/organization%20%2F%20one/forms/form%20%2F%20one/releases/release%20%2F%20one',
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/forms/form%20%2F%20one/releases', 'POST'],
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({ name: draft.name, description: '', document: draft.document }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'form:create',
        }),
      })
    );
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({
          name: revision.name,
          description: revision.description,
          document: revision.document,
        }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'form:revise',
          'x-a3s-expected-version': '1',
        }),
      })
    );
    expect(calls[6]?.[1]).toEqual(
      expect.objectContaining({
        body: undefined,
        headers: expect.objectContaining({
          'Idempotency-Key': 'form:publish',
          'x-a3s-expected-version': '2',
        }),
      })
    );
    expect((calls[6]?.[1]?.headers as Record<string, string>)['Content-Type']).toBeUndefined();
  });

  it('rejects invalid draft and version inputs before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    const valid = { name: 'Approval', document: {} };
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;

    expect(() =>
      api.createFormDraft('organization', 'project', { ...valid, name: '   ' }, 'form:create')
    ).toThrow('Form name must contain between 1 and 120 characters');
    expect(() =>
      api.createFormDraft(
        'organization',
        'project',
        { ...valid, description: 'x'.repeat(4_097) },
        'form:create'
      )
    ).toThrow('Form description must contain between 0 and 4096 characters');
    expect(() =>
      api.createFormDraft(
        'organization',
        'project',
        { ...valid, document: [] as unknown as Record<string, unknown> },
        'form:create'
      )
    ).toThrow('Form document must be a JSON object');
    expect(() =>
      api.createFormDraft('organization', 'project', { ...valid, document: cyclic }, 'form:create')
    ).toThrow('Form document must be JSON serializable');
    expect(() =>
      api.createFormDraft(
        'organization',
        'project',
        { ...valid, document: { value: 'x'.repeat(MAX_FORM_DOCUMENT_BYTES) } },
        'form:create'
      )
    ).toThrow(`Form document must contain at most ${MAX_FORM_DOCUMENT_BYTES} UTF-8 bytes`);
    expect(() =>
      api.reviseFormDraft('organization', 'form', valid, { expectedVersion: 0 }, 'form:revise')
    ).toThrow('expected Form draft version must be a positive safe integer');
    expect(() =>
      api.publishFormRelease('organization', 'form', { expectedVersion: 1.5 }, 'form:publish')
    ).toThrow('expected Form draft version must be a positive safe integer');
    expect(called).toBe(false);
  });
});
