import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';
import { MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES } from './developer-workflows';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const SOURCE_REVISION_ID = '019c0000-0000-7000-8000-000000000004';
const BUILD_PLAN_ID = '019c0000-0000-7000-8000-000000000005';
const PROPOSAL_ACL = 'build_plan { schema = "a3s.cloud.build-plan-proposal.v1" }\n';

describe('CloudApi Developer Workflows', () => {
  it('detects BuildPlans through a non-mutating JSON POST', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('token', '/api/v1', {
      fetch: async (...args) => {
        calls.push(args);
        return envelope({ source: {}, proposals: [], diagnostics: [] });
      },
    });

    await api.detectBuildPlans(ORGANIZATION_ID, PROJECT_ID, ENVIRONMENT_ID, {
      sourceRevisionId: SOURCE_REVISION_ID,
    });

    expect(calls[0]?.[0]).toBe(
      `/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
        `/environments/${ENVIRONMENT_ID}/build-plan-detections`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ sourceRevisionId: SOURCE_REVISION_ID }),
        headers: expect.objectContaining({ 'Content-Type': 'application/json' }),
      })
    );
    expect((calls[0]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBeUndefined();
  });

  it('accepts one proposal and reads exact accepted BuildPlan resources', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('token', '/api/v1', {
      fetch: async (...args) => {
        calls.push(args);
        return envelope({});
      },
    });

    await api.acceptBuildPlan(
      ORGANIZATION_ID,
      PROJECT_ID,
      ENVIRONMENT_ID,
      { sourceRevisionId: SOURCE_REVISION_ID, proposalAcl: PROPOSAL_ACL },
      'client:build-plan:accept'
    );
    await api.listAcceptedBuildPlans(ORGANIZATION_ID, PROJECT_ID, ENVIRONMENT_ID, SOURCE_REVISION_ID, 17);
    await api.getAcceptedBuildPlan(ORGANIZATION_ID, PROJECT_ID, ENVIRONMENT_ID, BUILD_PLAN_ID);

    const base =
      `/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}` +
      `/environments/${ENVIRONMENT_ID}/build-plans`;
    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [base, 'POST'],
      [`${base}?sourceRevisionId=${SOURCE_REVISION_ID}&limit=17`, 'GET'],
      [`${base}/${BUILD_PLAN_ID}`, 'GET'],
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({ sourceRevisionId: SOURCE_REVISION_ID, proposalAcl: PROPOSAL_ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'client:build-plan:accept',
        }),
      })
    );
  });

  it('rejects invalid proposal and page bounds before transport', async () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return envelope({});
      },
    });

    expect(() =>
      api.acceptBuildPlan(
        ORGANIZATION_ID,
        PROJECT_ID,
        ENVIRONMENT_ID,
        {
          sourceRevisionId: SOURCE_REVISION_ID,
          proposalAcl: 'a'.repeat(MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES + 1),
        },
        'client:build-plan:oversized'
      )
    ).toThrow(RangeError);
    expect(() =>
      api.listAcceptedBuildPlans(ORGANIZATION_ID, PROJECT_ID, ENVIRONMENT_ID, SOURCE_REVISION_ID, 201)
    ).toThrow(RangeError);
    expect(called).toBe(false);
  });
});

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-27T00:00:00.000Z',
    }),
    { status }
  );
}
