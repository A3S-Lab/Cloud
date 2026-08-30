import { describe, expect, it } from 'bun:test';
import {
  type CloudFetch,
  DEFAULT_WORKLOAD_TRUST_REVISION_LIST_LIMIT,
  MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT,
} from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const REVISION_ID = '019d0000-0000-7000-8000-000000000001';
const POLICY_ID = '019d0000-0000-7000-8000-000000000002';
const INSTALLATION_ID = '019d0000-0000-7000-8000-000000000003';
const BINDING_ID = '019d0000-0000-7000-8000-000000000004';
const PRINCIPAL_ID = '019d0000-0000-7000-8000-000000000005';
const ACTOR_ID = '019d0000-0000-7000-8000-000000000006';
const GRANT_ID = '019d0000-0000-7000-8000-000000000007';
const ORGANIZATION_ID = '019d0000-0000-7000-8000-000000000008';
const AUTHENTICATION_ID = '019d0000-0000-7000-8000-000000000009';
const APPROVAL_ID = '019d0000-0000-7000-8000-000000000010';
const TRUST_DOMAIN_ID = '019d0000-0000-7000-8000-000000000012';
const WORKLOAD_IDENTITY_POLICY_ID = '019d0000-0000-7000-8000-000000000013';
const WORKLOAD_ID = '019d0000-0000-7000-8000-000000000014';
const PROJECT_ID = '019d0000-0000-7000-8000-000000000015';
const ENVIRONMENT_ID = '019d0000-0000-7000-8000-000000000016';
const WORKLOAD_REVISION_ID = '019d0000-0000-7000-8000-000000000017';
const NODE_POOL_ID = '019d0000-0000-7000-8000-000000000018';
const POLICY_ACL = 'platform_role_policy { schema = "cloud.identity.platform-role-policy.v1" }\n';
const SUPPORT_ACL = 'tenant_support_grant { schema = "cloud.identity.tenant-support-grant.v1" }\n';
const TRUST_DOMAIN_ACL = 'trust_domain { schema = "cloud.identity.trust-domain.v1" }\n';
const WORKLOAD_IDENTITY_POLICY_ACL =
  'workload_identity_policy { schema = "cloud.identity.workload-identity-policy.v1" }\n';
const CONTRACT_DIGEST = `sha256:${'a'.repeat(64)}`;

describe('a3s-cloud privileged management commands', () => {
  it('queries all privileged authorities without requiring tenant context', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      if (path.includes('/role-policy')) {
        return envelope(platformRolePolicy());
      }
      if (path.includes('/role-bindings/') || path.includes('/principals/')) {
        return envelope(platformRoleBinding());
      }
      return envelope(tenantSupportGrant());
    };
    const output = capture();
    const runtime = { ...output.runtime, environment: tokenOnlyEnvironment(), fetch: fetcher };

    const commands = [
      ['platform-role-policy', 'current'],
      ['platform-role-policy', 'get', REVISION_ID],
      ['platform-role-bindings', 'get', BINDING_ID],
      ['platform-role-bindings', 'get-principal', PRINCIPAL_ID],
      ['tenant-support-grants', 'get', GRANT_ID],
    ];
    for (const command of commands) {
      expect(await runCli([...command, '--output=json'], runtime)).toBe(ExitCode.Success);
    }

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      ['http://127.0.0.1:8080/api/v1/platform/role-policy', 'GET'],
      [`http://127.0.0.1:8080/api/v1/platform/role-policy/revisions/${REVISION_ID}`, 'GET'],
      [`http://127.0.0.1:8080/api/v1/platform/role-bindings/${BINDING_ID}`, 'GET'],
      [`http://127.0.0.1:8080/api/v1/platform/principals/${PRINCIPAL_ID}/role-binding`, 'GET'],
      [`http://127.0.0.1:8080/api/v1/platform/tenant-support-grants/${GRANT_ID}`, 'GET'],
    ]);
    expect(output.stderr()).toBe('');
  });

  it('queries every workload trust projection with explicit bounded revision history', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      if (path.includes('/trust-domains/')) {
        if (path.endsWith('/provider-inspection')) {
          return envelope(workloadIdentityProviderInspection());
        }
        return envelope(path.includes('?limit=') ? [trustDomainRevision()] : trustDomainRevision());
      }
      return envelope(
        path.includes('?limit=') ? [workloadIdentityPolicyRevision()] : workloadIdentityPolicyRevision()
      );
    };
    const output = capture();
    const runtime = { ...output.runtime, environment: tokenOnlyEnvironment(), fetch: fetcher };

    const commands = [
      ['trust-domains', 'current', TRUST_DOMAIN_ID],
      ['trust-domains', 'inspect', TRUST_DOMAIN_ID],
      ['trust-domains', 'get', TRUST_DOMAIN_ID, REVISION_ID],
      ['trust-domains', 'list', TRUST_DOMAIN_ID],
      ['workload-identity-policies', 'current', ORGANIZATION_ID, WORKLOAD_IDENTITY_POLICY_ID],
      ['workload-identity-policies', 'get', ORGANIZATION_ID, WORKLOAD_IDENTITY_POLICY_ID, REVISION_ID],
      [
        'workload-identity-policies',
        'list',
        ORGANIZATION_ID,
        WORKLOAD_IDENTITY_POLICY_ID,
        `--limit=${MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT}`,
      ],
      ['workload-identity-policies', 'get-workload', ORGANIZATION_ID, WORKLOAD_ID],
    ];
    for (const command of commands) {
      expect(await runCli([...command, '--output=json'], runtime)).toBe(ExitCode.Success);
    }

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      [`http://127.0.0.1:8080/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}`, 'GET'],
      [`http://127.0.0.1:8080/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/provider-inspection`, 'GET'],
      [
        `http://127.0.0.1:8080/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/revisions/${REVISION_ID}`,
        'GET',
      ],
      [
        `http://127.0.0.1:8080/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/revisions?limit=${DEFAULT_WORKLOAD_TRUST_REVISION_LIST_LIMIT}`,
        'GET',
      ],
      [
        `http://127.0.0.1:8080/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}`,
        'GET',
      ],
      [
        `http://127.0.0.1:8080/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}/revisions/${REVISION_ID}`,
        'GET',
      ],
      [
        `http://127.0.0.1:8080/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}/revisions?limit=${MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT}`,
        'GET',
      ],
      [
        `http://127.0.0.1:8080/api/v1/platform/organizations/${ORGANIZATION_ID}/workloads/${WORKLOAD_ID}/identity-policy`,
        'GET',
      ],
    ]);
    expect(output.stderr()).toBe('');
  });

  it('accepts trust-domain and workload-policy ACL revisions with one predecessor fence', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const readPaths: string[] = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      return envelope(
        path.includes('/trust-domains/')
          ? { ...trustDomainRevision(), replayed: false }
          : { ...workloadIdentityPolicyRevision(), revisionNumber: 2, replayed: false },
        201
      );
    };
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: tokenOnlyEnvironment(),
      fetch: fetcher,
      readFile: async (path: string) => {
        readPaths.push(path);
        return new TextEncoder().encode(
          path === 'trust-domain.acl' ? TRUST_DOMAIN_ACL : WORKLOAD_IDENTITY_POLICY_ACL
        );
      },
    };

    expect(
      await runCli(
        [
          'trust-domains',
          'accept',
          TRUST_DOMAIN_ID,
          '1',
          'none',
          '--file=trust-domain.acl',
          '--idempotency-key=cli:trust-domain:1',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'workload-identity-policies',
          'accept',
          ORGANIZATION_ID,
          WORKLOAD_IDENTITY_POLICY_ID,
          '2',
          REVISION_ID,
          '--file=workload-identity-policy.acl',
          '--idempotency-key=cli:workload-policy:2',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);

    expect(readPaths).toEqual(['trust-domain.acl', 'workload-identity-policy.acl']);
    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/platform/trust-domains/${TRUST_DOMAIN_ID}/revisions`,
      `http://127.0.0.1:8080/api/v1/platform/organizations/${ORGANIZATION_ID}/workload-identity-policies/${WORKLOAD_IDENTITY_POLICY_ID}/revisions`,
    ]);
    expect(calls.map(([, init]) => init?.body)).toEqual([
      JSON.stringify({
        canonicalAcl: TRUST_DOMAIN_ACL,
        revisionNumber: 1,
        expectedPreviousRevisionId: null,
      }),
      JSON.stringify({
        canonicalAcl: WORKLOAD_IDENTITY_POLICY_ACL,
        revisionNumber: 2,
        expectedPreviousRevisionId: REVISION_ID,
      }),
    ]);
    expect(calls.map(([, init]) => (init?.headers as Record<string, string>)['Idempotency-Key'])).toEqual([
      'cli:trust-domain:1',
      'cli:workload-policy:2',
    ]);
    expect(output.stderr()).toBe('');
  });

  it('executes all policy-fenced and version-fenced mutations through the maintained client', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const readPaths: string[] = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      if (path.endsWith('/role-policy/revisions')) {
        return envelope({ ...platformRolePolicy(), revisionNumber: 2, replayed: false }, 201);
      }
      if (path.endsWith('/tenant-support-grants')) {
        return envelope({ proposal: tenantSupportProposal(), replayed: false }, 201);
      }
      if (path.endsWith('/approvals')) {
        return envelope(
          {
            outcome: {
              proposal: tenantSupportProposal(),
              approval: tenantSupportApproval(),
              grant: tenantSupportLifecycle(),
            },
            replayed: false,
          },
          201
        );
      }
      if (path.endsWith(`/tenant-support-grants/${GRANT_ID}/revocation`)) {
        return envelope({
          grant: { ...tenantSupportLifecycle(), revokedAt: '2026-08-29T02:00:00Z' },
          replayed: false,
        });
      }
      return envelope(
        { ...platformRoleBinding(), replayed: false },
        path.endsWith('/role-bindings') ? 201 : 200
      );
    };
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: tokenOnlyEnvironment(),
      fetch: fetcher,
      readFile: async (path: string) => {
        readPaths.push(path);
        return new TextEncoder().encode(path === 'policy.acl' ? POLICY_ACL : SUPPORT_ACL);
      },
    };

    const commands = [
      [
        'platform-role-policy',
        'accept',
        '2',
        REVISION_ID,
        '--file=policy.acl',
        '--idempotency-key=cli:platform-policy:2',
      ],
      [
        'platform-role-bindings',
        'create',
        PRINCIPAL_ID,
        'platform_operator',
        REVISION_ID,
        '--idempotency-key=cli:platform-binding:create',
      ],
      [
        'platform-role-bindings',
        'change-role',
        BINDING_ID,
        'security_auditor',
        REVISION_ID,
        '--expected-version=1',
        '--idempotency-key=cli:platform-binding:change-role',
      ],
      [
        'platform-role-bindings',
        'revoke',
        BINDING_ID,
        '--expected-version=2',
        '--idempotency-key=cli:platform-binding:revoke',
      ],
      [
        'tenant-support-grants',
        'propose',
        '--file=support.acl',
        '--idempotency-key=cli:tenant-support:propose',
      ],
      [
        'tenant-support-grants',
        'approve',
        GRANT_ID,
        CONTRACT_DIGEST,
        '--idempotency-key=cli:tenant-support:approve',
      ],
      [
        'tenant-support-grants',
        'revoke',
        GRANT_ID,
        '--expected-version=1',
        '--idempotency-key=cli:tenant-support:revoke',
      ],
    ];
    for (const command of commands) {
      expect(await runCli(command, runtime)).toBe(ExitCode.Success);
    }

    expect(readPaths).toEqual(['policy.acl', 'support.acl']);
    expect(calls.map(([input]) => input)).toEqual([
      'http://127.0.0.1:8080/api/v1/platform/role-policy/revisions',
      'http://127.0.0.1:8080/api/v1/platform/role-bindings',
      `http://127.0.0.1:8080/api/v1/platform/role-bindings/${BINDING_ID}/role`,
      `http://127.0.0.1:8080/api/v1/platform/role-bindings/${BINDING_ID}/revocation`,
      'http://127.0.0.1:8080/api/v1/platform/tenant-support-grants',
      `http://127.0.0.1:8080/api/v1/platform/tenant-support-grants/${GRANT_ID}/approvals`,
      `http://127.0.0.1:8080/api/v1/platform/tenant-support-grants/${GRANT_ID}/revocation`,
    ]);
    expect(calls.map(([, init]) => init?.body)).toEqual([
      JSON.stringify({ canonicalAcl: POLICY_ACL, revisionNumber: 2, expectedCurrentRevisionId: REVISION_ID }),
      JSON.stringify({
        principalId: PRINCIPAL_ID,
        role: 'platform_operator',
        expectedPolicyRevisionId: REVISION_ID,
      }),
      JSON.stringify({ role: 'security_auditor', expectedVersion: 1, expectedPolicyRevisionId: REVISION_ID }),
      JSON.stringify({ expectedVersion: 2 }),
      JSON.stringify({ canonicalAcl: SUPPORT_ACL }),
      JSON.stringify({ expectedContractDigest: CONTRACT_DIGEST }),
      JSON.stringify({ expectedVersion: 1 }),
    ]);
    expect(calls.map(([, init]) => (init?.headers as Record<string, string>)['Idempotency-Key'])).toEqual([
      'cli:platform-policy:2',
      'cli:platform-binding:create',
      'cli:platform-binding:change-role',
      'cli:platform-binding:revoke',
      'cli:tenant-support:propose',
      'cli:tenant-support:approve',
      'cli:tenant-support:revoke',
    ]);
    expect(output.stderr()).toBe('');
  });

  it('rejects malformed privileged intent before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return envelope({});
    };
    const cases = [
      {
        argv: [
          'platform-role-policy',
          'accept',
          '0',
          REVISION_ID,
          '--file=policy.acl',
          '--idempotency-key=k',
        ],
        message: 'revision number must be a positive safe integer',
      },
      {
        argv: [
          'platform-role-policy',
          'accept',
          '2',
          REVISION_ID,
          '--file=policy.json',
          '--idempotency-key=k',
        ],
        message: 'must reference a .acl file',
      },
      {
        argv: [
          'platform-role-bindings',
          'create',
          PRINCIPAL_ID,
          'tenant_admin',
          REVISION_ID,
          '--idempotency-key=k',
        ],
        message: 'closed platform roles',
      },
      {
        argv: ['platform-role-bindings', 'revoke', BINDING_ID, '--idempotency-key=k'],
        message: '--expected-version must be a positive safe integer',
      },
      {
        argv: ['tenant-support-grants', 'approve', GRANT_ID, 'sha256:not-a-digest', '--idempotency-key=k'],
        message: 'canonical SHA-256 digest',
      },
      {
        argv: ['tenant-support-grants', 'propose', '--file=support.acl'],
        message: '--idempotency-key is required',
      },
      {
        argv: [
          'trust-domains',
          'accept',
          TRUST_DOMAIN_ID,
          '1',
          REVISION_ID,
          '--file=trust-domain.acl',
          '--idempotency-key=k',
        ],
        message: 'revision 1 must use none',
      },
      {
        argv: [
          'workload-identity-policies',
          'accept',
          ORGANIZATION_ID,
          WORKLOAD_IDENTITY_POLICY_ID,
          '2',
          'none',
          '--file=workload-identity-policy.acl',
          '--idempotency-key=k',
        ],
        message: 'requires a previous revision ID',
      },
      {
        argv: [
          'workload-identity-policies',
          'list',
          ORGANIZATION_ID,
          WORKLOAD_IDENTITY_POLICY_ID,
          `--limit=${MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT + 1}`,
        ],
        message: '--limit must be between 1 and',
      },
    ];

    for (const testCase of cases) {
      const output = capture();
      const exitCode = await runCli(testCase.argv, {
        ...output.runtime,
        environment: tokenOnlyEnvironment(),
        fetch: fetcher,
        readFile: async () => new TextEncoder().encode(POLICY_ACL),
      });
      expect(exitCode).toBe(ExitCode.Usage);
      expect(output.stderr()).toContain(testCase.message);
    }
    expect(called).toBe(false);
  });
});

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019d0000-0000-7000-8000-000000000011',
      timestamp: '2026-08-29T00:00:00.000Z',
    }),
    { status }
  );
}

function capture() {
  let stdout = '';
  let stderr = '';
  return {
    runtime: {
      writeStdout: (value: string) => {
        stdout += value;
      },
      writeStderr: (value: string) => {
        stderr += value;
      },
    },
    stdout: () => stdout,
    stderr: () => stderr,
  };
}

function tokenOnlyEnvironment() {
  return { A3S_CLOUD_TOKEN: 'caller-token' };
}

function platformRolePolicy() {
  return {
    id: REVISION_ID,
    installationId: INSTALLATION_ID,
    policyId: POLICY_ID,
    revisionNumber: 1,
    canonicalAcl: POLICY_ACL,
    digest: `sha256:${'b'.repeat(64)}`,
    rolePermissions: [{ role: 'platform_owner', permissions: ['platform:read'] }],
    acceptedBy: ACTOR_ID,
    acceptedAt: '2026-08-29T00:00:00Z',
  };
}

function trustDomainRevision() {
  return {
    installationId: INSTALLATION_ID,
    trustDomainId: TRUST_DOMAIN_ID,
    revisionId: REVISION_ID,
    revisionNumber: 1,
    name: 'cluster.example.test',
    canonicalAcl: TRUST_DOMAIN_ACL,
    digest: `sha256:${'e'.repeat(64)}`,
    acceptedBy: ACTOR_ID,
    acceptedAt: '2026-08-29T00:00:00Z',
  };
}

function workloadIdentityPolicyRevision() {
  return {
    installationId: INSTALLATION_ID,
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    policyId: WORKLOAD_IDENTITY_POLICY_ID,
    revisionId: REVISION_ID,
    revisionNumber: 1,
    trustDomainId: TRUST_DOMAIN_ID,
    trustDomainRevisionId: REVISION_ID,
    workloadId: WORKLOAD_ID,
    workloadRevisionId: WORKLOAD_REVISION_ID,
    nodePoolId: NODE_POOL_ID,
    canonicalAcl: WORKLOAD_IDENTITY_POLICY_ACL,
    digest: `sha256:${'f'.repeat(64)}`,
    acceptedBy: ACTOR_ID,
    acceptedAt: '2026-08-29T00:00:00Z',
  };
}

function workloadIdentityProviderInspection() {
  return {
    revision: trustDomainRevision(),
    providerProfileDigest: `sha256:${'a'.repeat(64)}`,
    trustDomainName: 'cluster.example.test',
    observedTrustBundleDigest: `sha256:${'b'.repeat(64)}`,
    observedFederationBundleDigests: [],
    observedIdentityFormats: ['x509_svid'],
    declaredNodeAttestationProfileDigests: [`sha256:${'c'.repeat(64)}`],
    declaredMaxCredentialLifetimeSeconds: 900,
    declaredSupportsRevocationEpochs: false,
    observedAt: '2026-08-29T00:01:00Z',
  };
}

function platformRoleBinding() {
  return {
    id: BINDING_ID,
    installationId: INSTALLATION_ID,
    principalId: PRINCIPAL_ID,
    role: 'platform_operator',
    aggregateVersion: 1,
    createdBy: ACTOR_ID,
    updatedBy: ACTOR_ID,
    createdAt: '2026-08-29T00:00:00Z',
    updatedAt: '2026-08-29T00:00:00Z',
    revokedAt: null,
  };
}

function tenantSupportProposal() {
  return {
    id: GRANT_ID,
    principalId: PRINCIPAL_ID,
    scope: {
      kind: 'organization',
      installationId: INSTALLATION_ID,
      organizationId: ORGANIZATION_ID,
      projectId: null,
      environmentId: null,
    },
    permissions: ['tenant-support:health:read'],
    caseReference: 'CASE-42',
    justificationDigest: `sha256:${'c'.repeat(64)}`,
    mode: 'standard',
    approvalRequirement: 'single',
    approverIds: [ACTOR_ID],
    tenantNotification: 'required',
    securityAlertRequired: false,
    postIncidentReviewRequired: false,
    startsAt: '2026-08-29T00:00:00Z',
    expiresAt: '2026-08-29T01:00:00Z',
    canonicalAcl: SUPPORT_ACL,
    contractDigest: CONTRACT_DIGEST,
    requestedBy: ACTOR_ID,
    authentication: { id: AUTHENTICATION_ID, digest: `sha256:${'d'.repeat(64)}` },
    requestedAt: '2026-08-29T00:00:00Z',
  };
}

function tenantSupportApproval() {
  return {
    grantId: GRANT_ID,
    contractDigest: CONTRACT_DIGEST,
    approverId: ACTOR_ID,
    authentication: { id: AUTHENTICATION_ID, digest: `sha256:${'d'.repeat(64)}` },
    policyRevisionId: REVISION_ID,
    policyDigest: `sha256:${'b'.repeat(64)}`,
    bindingId: BINDING_ID,
    bindingVersion: 1,
    approvedAt: '2026-08-29T00:01:00Z',
    digest: APPROVAL_ID,
  };
}

function tenantSupportLifecycle() {
  return {
    id: GRANT_ID,
    aggregateVersion: 1,
    revocationGeneration: 0,
    acceptedAt: '2026-08-29T00:01:00Z',
    revokedAt: null,
    revokedBy: null,
  };
}

function tenantSupportGrant() {
  return {
    proposal: tenantSupportProposal(),
    approvals: [tenantSupportApproval()],
    grant: tenantSupportLifecycle(),
  };
}
