import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
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
const POLICY_ACL = 'platform_role_policy { schema = "cloud.identity.platform-role-policy.v1" }\n';
const SUPPORT_ACL = 'tenant_support_grant { schema = "cloud.identity.tenant-support-grant.v1" }\n';
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
