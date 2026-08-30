import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';
import {
  DEFAULT_WORKLOAD_TRUST_REVISION_LIST_LIMIT,
  MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT,
  PLATFORM_ROLE_POLICY_MAX_ACL_BYTES,
  TENANT_SUPPORT_GRANT_MAX_ACL_BYTES,
  TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES,
  WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES,
} from './privileged-management';

const REVISION_ID = '019c0000-0000-7000-8000-000000000001';
const BINDING_ID = '019c0000-0000-7000-8000-000000000002';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000003';
const GRANT_ID = '019c0000-0000-7000-8000-000000000004';
const TRUST_DOMAIN_ID = '019c0000-0000-7000-8000-000000000006';
const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000007';
const WORKLOAD_IDENTITY_POLICY_ID = '019c0000-0000-7000-8000-000000000008';
const WORKLOAD_ID = '019c0000-0000-7000-8000-000000000009';
const POLICY_ACL = 'platform_role_policy { schema = "cloud.identity.platform-role-policy.v1" }\n';
const SUPPORT_ACL = 'tenant_support_grant { schema = "cloud.identity.tenant-support-grant.v1" }\n';
const TRUST_DOMAIN_ACL = 'trust_domain { schema = "cloud.identity.trust-domain.v1" }\n';
const WORKLOAD_IDENTITY_POLICY_ACL =
  'workload_identity_policy { schema = "cloud.identity.workload-identity-policy.v1" }\n';
const CONTRACT_DIGEST = `sha256:${'a'.repeat(64)}`;

function envelope(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000005',
      timestamp: '2026-08-29T08:00:00.000Z',
    }),
    { status: 200, headers: { 'content-type': 'application/json' } }
  );
}

describe('CloudApi privileged management', () => {
  it('uses the exact closed read routes without caller-authored authority', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('credential', '/api/v1', {
      fetch: async (...arguments_) => {
        calls.push(arguments_);
        return envelope({});
      },
    });

    await api.getCurrentPlatformRolePolicy();
    await api.getPlatformRolePolicyRevision(REVISION_ID);
    await api.getPlatformRoleBinding(BINDING_ID);
    await api.getPrincipalPlatformRoleBinding(PRINCIPAL_ID);
    await api.getTenantSupportGrant(GRANT_ID);

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      ['/api/v1/platform/role-policy', 'GET'],
      [`/api/v1/platform/role-policy/revisions/${REVISION_ID}`, 'GET'],
      [`/api/v1/platform/role-bindings/${BINDING_ID}`, 'GET'],
      [`/api/v1/platform/principals/${PRINCIPAL_ID}/role-binding`, 'GET'],
      [`/api/v1/platform/tenant-support-grants/${GRANT_ID}`, 'GET'],
    ]);
    for (const [, init] of calls) {
      expect(init?.body).toBeUndefined();
      expect(init?.headers).toEqual(
        expect.objectContaining({ Authorization: 'Bearer credential', Accept: 'application/json' })
      );
    }
  });

  it('reads current, exact, workload-indexed, and bounded workload trust revisions', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('credential', '/api/v1', {
      fetch: async (...arguments_) => {
        calls.push(arguments_);
        return envelope({});
      },
    });

    await api.getCurrentTrustDomain(TRUST_DOMAIN_ID);
    await api.inspectCurrentTrustDomainProvider(TRUST_DOMAIN_ID);
    await api.getTrustDomainRevision(TRUST_DOMAIN_ID, REVISION_ID);
    await api.listTrustDomainRevisions(TRUST_DOMAIN_ID);
    await api.getCurrentWorkloadIdentityPolicy(ORGANIZATION_ID, WORKLOAD_IDENTITY_POLICY_ID);
    await api.getCurrentWorkloadIdentityPolicyForWorkload(ORGANIZATION_ID, WORKLOAD_ID);
    await api.getWorkloadIdentityPolicyRevision(ORGANIZATION_ID, WORKLOAD_IDENTITY_POLICY_ID, REVISION_ID);
    await api.listWorkloadIdentityPolicyRevisions(ORGANIZATION_ID, WORKLOAD_IDENTITY_POLICY_ID, {
      limit: MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT,
    });

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [`/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}`, 'GET'],
      [`/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/provider-inspection`, 'GET'],
      [`/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/revisions/${REVISION_ID}`, 'GET'],
      [
        `/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/revisions?limit=${DEFAULT_WORKLOAD_TRUST_REVISION_LIST_LIMIT}`,
        'GET',
      ],
      [
        `/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}`,
        'GET',
      ],
      [`/api/v1/platform/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}/identity-policy`, 'GET'],
      [
        `/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}/revisions/${REVISION_ID}`,
        'GET',
      ],
      [
        `/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}/revisions?limit=${MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT}`,
        'GET',
      ],
    ]);
    for (const [, init] of calls) {
      expect(init?.body).toBeUndefined();
      expect(init?.headers).toEqual(
        expect.objectContaining({ Authorization: 'Bearer credential', Accept: 'application/json' })
      );
    }
  });

  it('accepts workload trust revisions with immutable predecessor control and one replay key', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('credential', '/api/v1', {
      fetch: async (...arguments_) => {
        calls.push(arguments_);
        return envelope({});
      },
    });

    await api.acceptTrustDomainRevision(
      TRUST_DOMAIN_ID,
      { canonicalAcl: TRUST_DOMAIN_ACL, revisionNumber: 1, expectedPreviousRevisionId: null },
      'client:trust-domain:1'
    );
    await api.acceptWorkloadIdentityPolicyRevision(
      ORGANIZATION_ID,
      WORKLOAD_IDENTITY_POLICY_ID,
      {
        canonicalAcl: WORKLOAD_IDENTITY_POLICY_ACL,
        revisionNumber: 2,
        expectedPreviousRevisionId: REVISION_ID,
      },
      'client:workload-identity-policy:2'
    );

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [`/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/revisions`, 'POST'],
      [
        `/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}/revisions`,
        'POST',
      ],
    ]);
    expect(calls.map(([, init]) => (init?.headers as Record<string, string>)['Idempotency-Key'])).toEqual([
      'client:trust-domain:1',
      'client:workload-identity-policy:2',
    ]);
    expect(calls.map(([, init]) => JSON.parse(String(init?.body)))).toEqual([
      { canonicalAcl: TRUST_DOMAIN_ACL, revisionNumber: 1, expectedPreviousRevisionId: null },
      {
        canonicalAcl: WORKLOAD_IDENTITY_POLICY_ACL,
        revisionNumber: 2,
        expectedPreviousRevisionId: REVISION_ID,
      },
    ]);
    for (const [, init] of calls) {
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      expect(body.actorPrincipalId).toBeUndefined();
      expect(body.credentialId).toBeUndefined();
      expect(body.installationId).toBeUndefined();
    }
  });

  it('transports all seven mutations with one replay key and closed domain inputs', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('credential', '/api/v1', {
      fetch: async (...arguments_) => {
        calls.push(arguments_);
        return envelope({});
      },
    });

    await api.acceptPlatformRolePolicy(
      { canonicalAcl: POLICY_ACL, revisionNumber: 2, expectedCurrentRevisionId: REVISION_ID },
      'client:platform-policy:2'
    );
    await api.createPlatformRoleBinding(
      { principalId: PRINCIPAL_ID, role: 'platform_operator', expectedPolicyRevisionId: REVISION_ID },
      'client:platform-binding:create'
    );
    await api.changePlatformRoleBinding(
      BINDING_ID,
      { role: 'security_auditor', expectedVersion: 1, expectedPolicyRevisionId: REVISION_ID },
      'client:platform-binding:change'
    );
    await api.revokePlatformRoleBinding(BINDING_ID, 2, 'client:platform-binding:revoke');
    await api.proposeTenantSupportGrant({ canonicalAcl: SUPPORT_ACL }, 'client:tenant-support:propose');
    await api.approveTenantSupportGrant(
      GRANT_ID,
      { expectedContractDigest: CONTRACT_DIGEST },
      'client:tenant-support:approve'
    );
    await api.revokeTenantSupportGrant(GRANT_ID, 1, 'client:tenant-support:revoke');

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      ['/api/v1/platform/role-policy/revisions', 'POST'],
      ['/api/v1/platform/role-bindings', 'POST'],
      [`/api/v1/platform/role-bindings/${BINDING_ID}/role`, 'POST'],
      [`/api/v1/platform/role-bindings/${BINDING_ID}/revocation`, 'POST'],
      ['/api/v1/platform/tenant-support-grants', 'POST'],
      [`/api/v1/platform/tenant-support-grants/${GRANT_ID}/approvals`, 'POST'],
      [`/api/v1/platform/tenant-support-grants/${GRANT_ID}/revocation`, 'POST'],
    ]);
    expect(calls.map(([, init]) => (init?.headers as Record<string, string>)['Idempotency-Key'])).toEqual([
      'client:platform-policy:2',
      'client:platform-binding:create',
      'client:platform-binding:change',
      'client:platform-binding:revoke',
      'client:tenant-support:propose',
      'client:tenant-support:approve',
      'client:tenant-support:revoke',
    ]);
    expect(calls.map(([, init]) => JSON.parse(String(init?.body)))).toEqual([
      { canonicalAcl: POLICY_ACL, revisionNumber: 2, expectedCurrentRevisionId: REVISION_ID },
      { principalId: PRINCIPAL_ID, role: 'platform_operator', expectedPolicyRevisionId: REVISION_ID },
      { role: 'security_auditor', expectedVersion: 1, expectedPolicyRevisionId: REVISION_ID },
      { expectedVersion: 2 },
      { canonicalAcl: SUPPORT_ACL },
      { expectedContractDigest: CONTRACT_DIGEST },
      { expectedVersion: 1 },
    ]);
    for (const [, init] of calls) {
      expect(init?.headers).toEqual(expect.objectContaining({ 'Content-Type': 'application/json' }));
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      expect(body.actorPrincipalId).toBeUndefined();
      expect(body.credentialId).toBeUndefined();
      expect(body.installationId).toBeUndefined();
    }
  });

  it('rejects malformed authority identities, versions, roles, digests, and ACL bounds before transport', () => {
    let called = false;
    const api = new CloudApi('credential', '/api/v1', {
      fetch: async () => {
        called = true;
        return envelope({});
      },
    });

    expect(() => api.getPlatformRoleBinding('not-a-uuid')).toThrow('non-nil UUID');
    expect(() =>
      api.acceptPlatformRolePolicy(
        { canonicalAcl: POLICY_ACL, revisionNumber: 0, expectedCurrentRevisionId: REVISION_ID },
        'invalid-version'
      )
    ).toThrow('positive safe integer');
    expect(() =>
      api.acceptPlatformRolePolicy(
        {
          canonicalAcl: 'a'.repeat(PLATFORM_ROLE_POLICY_MAX_ACL_BYTES + 1),
          revisionNumber: 2,
          expectedCurrentRevisionId: REVISION_ID,
        },
        'oversized-policy'
      )
    ).toThrow(RangeError);
    expect(() =>
      api.createPlatformRoleBinding(
        {
          principalId: PRINCIPAL_ID,
          role: 'tenant_admin' as never,
          expectedPolicyRevisionId: REVISION_ID,
        },
        'forged-role'
      )
    ).toThrow('closed platform roles');
    expect(() => api.revokePlatformRoleBinding(BINDING_ID, 0, 'invalid-version')).toThrow(
      'positive safe integer'
    );
    expect(() =>
      api.proposeTenantSupportGrant(
        { canonicalAcl: 'a'.repeat(TENANT_SUPPORT_GRANT_MAX_ACL_BYTES + 1) },
        'oversized-support'
      )
    ).toThrow(RangeError);
    expect(() =>
      api.approveTenantSupportGrant(
        GRANT_ID,
        { expectedContractDigest: 'sha256:not-a-digest' },
        'invalid-digest'
      )
    ).toThrow('canonical SHA-256 digest');
    expect(() => api.getCurrentTrustDomain('not-a-uuid')).toThrow('non-nil UUID');
    expect(() =>
      api.acceptTrustDomainRevision(
        TRUST_DOMAIN_ID,
        {
          canonicalAcl: TRUST_DOMAIN_ACL,
          revisionNumber: 1,
          expectedPreviousRevisionId: REVISION_ID,
        },
        'forged-predecessor'
      )
    ).toThrow('must not declare');
    expect(() =>
      api.acceptTrustDomainRevision(
        TRUST_DOMAIN_ID,
        {
          canonicalAcl: 'a'.repeat(TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES + 1),
          revisionNumber: 1,
        },
        'oversized-trust-domain'
      )
    ).toThrow(RangeError);
    expect(() =>
      api.acceptWorkloadIdentityPolicyRevision(
        ORGANIZATION_ID,
        WORKLOAD_IDENTITY_POLICY_ID,
        { canonicalAcl: WORKLOAD_IDENTITY_POLICY_ACL, revisionNumber: 2 },
        'missing-predecessor'
      )
    ).toThrow('non-nil UUID');
    expect(() =>
      api.acceptWorkloadIdentityPolicyRevision(
        ORGANIZATION_ID,
        WORKLOAD_IDENTITY_POLICY_ID,
        {
          canonicalAcl: 'a'.repeat(WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES + 1),
          revisionNumber: 2,
          expectedPreviousRevisionId: REVISION_ID,
        },
        'oversized-workload-policy'
      )
    ).toThrow(RangeError);
    expect(() =>
      api.listWorkloadIdentityPolicyRevisions(ORGANIZATION_ID, WORKLOAD_IDENTITY_POLICY_ID, {
        limit: MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT + 1,
      })
    ).toThrow('must be between 1 and');
    expect(called).toBe(false);
  });
});
