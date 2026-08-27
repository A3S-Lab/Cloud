import { describe, expect, it } from 'bun:test';
import {
  type BuildPlanDetection,
  type CloudFetch,
  MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES,
  MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER,
  MAX_PULL_REQUEST_PREVIEW_POLICY_ACL_BYTES,
  MAX_WORKLOAD_PROFILE_ACL_BYTES,
} from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const SOURCE_REVISION_ID = '019c0000-0000-7000-8000-000000000004';
const BUILD_PLAN_ID = '019c0000-0000-7000-8000-000000000005';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000006';
const WORKLOAD_PROFILE_ID = '019c0000-0000-7000-8000-000000000007';
const WORKLOAD_PROFILE_REVISION_ID = '019c0000-0000-7000-8000-000000000008';
const SOURCE_SUBSCRIPTION_ID = '019c0000-0000-7000-8000-000000000009';
const PREVIEW_POLICY_REVISION_ID = '019c0000-0000-7000-8000-00000000000a';
const PREVIEW_ID = '019c0000-0000-7000-8000-00000000000b';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const COMMIT = 'b'.repeat(40);
const PROPOSAL_ACL = 'build_plan { schema = "a3s.cloud.build-plan-proposal.v1" detector = "dockerfile" }\n';
const PROFILE_ACL = 'workload_profile { schema = "a3s.cloud.workload-profile.v1" }\n';
const PREVIEW_POLICY_ACL =
  'pull_request_preview_policy { schema = "a3s.cloud.pull-request-preview-policy.v1" }\n';

describe('a3s-cloud BuildPlan commands', () => {
  it('detects proposals as a read-only POST without an idempotency header', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(['build-plan-detections', 'create', SOURCE_REVISION_ID, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(detection());
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/build-plan-detections`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ sourceRevisionId: SOURCE_REVISION_ID }),
        headers: expect.objectContaining({ 'Content-Type': 'application/json' }),
      })
    );
    expect((calls[0]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBeUndefined();
    expect(output.stderr()).toBe('');
  });

  it('lists and gets accepted BuildPlans through exact environment scope', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        return envelope(String(args[0]).includes('?') ? [acceptedPlan()] : acceptedPlan());
      },
    };

    expect(
      await runCli(['build-plans', 'list', SOURCE_REVISION_ID, '--limit=2', '--output=json'], runtime)
    ).toBe(ExitCode.Success);
    expect(await runCli(['build-plans', 'get', BUILD_PLAN_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );

    const base =
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
      `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/build-plans`;
    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [`${base}?sourceRevisionId=${SOURCE_REVISION_ID}&limit=2`, 'GET'],
      [`${base}/${BUILD_PLAN_ID}`, 'GET'],
    ]);
    expect(output.stderr()).toBe('');
  });

  it('accepts only a bounded proposal ACL with caller-owned idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'build-plans',
        'accept',
        SOURCE_REVISION_ID,
        '--file=proposal.acl',
        '--idempotency-key=cli:build-plan:accept',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('proposal.acl');
          return new TextEncoder().encode(PROPOSAL_ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ buildPlan: acceptedPlan(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
        `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/build-plans`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ sourceRevisionId: SOURCE_REVISION_ID, proposalAcl: PROPOSAL_ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:build-plan:accept',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects oversized ACL and out-of-range list bounds before transport', async () => {
    let called = false;
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async () => new Uint8Array(MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES + 1),
      fetch: async () => {
        called = true;
        return envelope({});
      },
    };

    expect(
      await runCli(
        [
          'build-plans',
          'accept',
          SOURCE_REVISION_ID,
          '--file=proposal.acl',
          '--idempotency-key=cli:build-plan:oversized',
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('BuildPlan proposal ACL must contain between');

    expect(await runCli(['build-plans', 'list', SOURCE_REVISION_ID, '--limit=201'], runtime)).toBe(
      ExitCode.Usage
    );
    expect(output.stderr()).toContain('BuildPlan list limit must be between 1 and 200');
    expect(called).toBe(false);
  });

  it('accepts one ACL-only WorkloadProfile revision with caller-owned idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'workload-profiles',
        'accept',
        BUILD_PLAN_ID,
        '--file=profile.acl',
        '--idempotency-key=cli:workload-profile:accept',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('profile.acl');
          return new TextEncoder().encode(PROFILE_ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ workloadProfileRevision: acceptedProfileRevision(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`${workloadProfileBase()}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ buildPlanId: BUILD_PLAN_ID, profileAcl: PROFILE_ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:workload-profile:accept',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('gets current, lists history, and gets one exact WorkloadProfile revision', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        return envelope(
          String(args[0]).includes('/revisions?') ? [acceptedProfileRevision()] : acceptedProfileRevision()
        );
      },
    };

    expect(await runCli(['workload-profiles', 'get', WORKLOAD_PROFILE_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(
      await runCli(
        ['workload-profile-revisions', 'list', WORKLOAD_PROFILE_ID, '--limit=2', '--output=json'],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'workload-profile-revisions',
          'get',
          WORKLOAD_PROFILE_ID,
          WORKLOAD_PROFILE_REVISION_ID,
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);

    const base = workloadProfileBase();
    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [`${base}/${WORKLOAD_PROFILE_ID}`, 'GET'],
      [`${base}/${WORKLOAD_PROFILE_ID}/revisions?limit=2`, 'GET'],
      [`${base}/${WORKLOAD_PROFILE_ID}/revisions/${WORKLOAD_PROFILE_REVISION_ID}`, 'GET'],
    ]);
    expect(output.stderr()).toBe('');
  });

  it('rejects oversized WorkloadProfile ACL and revision bounds before transport', async () => {
    let called = false;
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async () => new Uint8Array(MAX_WORKLOAD_PROFILE_ACL_BYTES + 1),
      fetch: async () => {
        called = true;
        return envelope({});
      },
    };

    expect(
      await runCli(
        [
          'workload-profiles',
          'accept',
          BUILD_PLAN_ID,
          '--file=profile.acl',
          '--idempotency-key=cli:workload-profile:oversized',
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('WorkloadProfile ACL must contain between');
    expect(
      await runCli(['workload-profile-revisions', 'list', WORKLOAD_PROFILE_ID, '--limit=101'], runtime)
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('WorkloadProfile revision list limit must be between 1 and 100');
    expect(called).toBe(false);
  });

  it('accepts one ACL-only Preview Policy revision with caller-owned idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'preview-policies',
        'accept',
        SOURCE_SUBSCRIPTION_ID,
        '--file=preview-policy.acl',
        '--idempotency-key=cli:preview-policy:accept',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('preview-policy.acl');
          return new TextEncoder().encode(PREVIEW_POLICY_ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ previewPolicyRevision: acceptedPreviewPolicyRevision(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(previewPolicyCollection());
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          sourceSubscriptionId: SOURCE_SUBSCRIPTION_ID,
          policyAcl: PREVIEW_POLICY_ACL,
        }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:preview-policy:accept',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('gets current policy, bounded history, exact revision, and one PR Preview', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        const request = String(args[0]);
        if (request.includes('/pull-request-previews/')) {
          return envelope(pullRequestPreview());
        }
        return envelope(
          request.includes('/revisions?')
            ? [acceptedPreviewPolicyRevision()]
            : acceptedPreviewPolicyRevision()
        );
      },
    };

    expect(await runCli(['preview-policies', 'get', SOURCE_SUBSCRIPTION_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(
      await runCli(
        ['preview-policy-revisions', 'list', SOURCE_SUBSCRIPTION_ID, '--limit=2', '--output=json'],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'preview-policy-revisions',
          'get',
          SOURCE_SUBSCRIPTION_ID,
          PREVIEW_POLICY_REVISION_ID,
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(['pull-request-previews', 'get', SOURCE_SUBSCRIPTION_ID, '42', '--output=json'], runtime)
    ).toBe(ExitCode.Success);

    const policy = `${previewPolicyCollection()}/${SOURCE_SUBSCRIPTION_ID}`;
    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [policy, 'GET'],
      [`${policy}/revisions?limit=2`, 'GET'],
      [`${policy}/revisions/${PREVIEW_POLICY_REVISION_ID}`, 'GET'],
      [
        `${developerWorkflowEnvironmentBase()}/pull-request-previews/${SOURCE_SUBSCRIPTION_ID}` +
          '/pull-requests/42',
        'GET',
      ],
    ]);
    expect(output.stderr()).toBe('');
  });

  it('rejects non-ACL Preview files, oversized ACL, history bounds, and PR IDs before transport', async () => {
    let called = false;
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async () => new Uint8Array(MAX_PULL_REQUEST_PREVIEW_POLICY_ACL_BYTES + 1),
      fetch: async () => {
        called = true;
        return envelope({});
      },
    };

    expect(
      await runCli(
        [
          'preview-policies',
          'accept',
          SOURCE_SUBSCRIPTION_ID,
          '--file=preview-policy.json',
          '--idempotency-key=cli:preview-policy:wrong-file',
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('.acl');
    expect(
      await runCli(
        [
          'preview-policies',
          'accept',
          SOURCE_SUBSCRIPTION_ID,
          '--file=preview-policy.acl',
          '--idempotency-key=cli:preview-policy:oversized',
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('Pull-request Preview Policy ACL must contain between');
    expect(
      await runCli(['preview-policy-revisions', 'list', SOURCE_SUBSCRIPTION_ID, '--limit=101'], runtime)
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain(
      'Pull-request Preview Policy revision list limit must be between 1 and 100'
    );
    expect(
      await runCli(
        [
          'pull-request-previews',
          'get',
          SOURCE_SUBSCRIPTION_ID,
          String(MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1),
        ],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('Pull-request ID must be a portable positive integer');
    expect(called).toBe(false);
  });
});

function detection(): BuildPlanDetection {
  return {
    source: {
      sourceIdentityDigest: DIGEST,
      commitSha: COMMIT,
      sourceContentDigest: DIGEST,
    },
    proposals: [proposal()],
    diagnostics: [],
  };
}

function proposal() {
  return {
    schema: 'a3s.cloud.build-plan-proposal.v1' as const,
    proposalAcl: PROPOSAL_ACL,
    proposalDigest: DIGEST,
    detector: 'dockerfile' as const,
    detectorRevision: 'p0.1-c1' as const,
    projectRoot: '.',
    evidencePath: 'Dockerfile',
    evidenceDigest: DIGEST,
    recipe: {
      schema: 'a3s.cloud.build-recipe.v1' as const,
      kind: 'dockerfile' as const,
      contextPath: '.',
      dockerfilePath: 'Dockerfile',
      target: null,
      platforms: ['linux/amd64' as const],
    },
  };
}

function acceptedPlan() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    buildPlanId: BUILD_PLAN_ID,
    sourceRevisionId: SOURCE_REVISION_ID,
    contractSchema: 'a3s.cloud.build-plan.v1' as const,
    contractAcl: 'build_plan { schema = "a3s.cloud.build-plan.v1" }\n',
    contractDigest: DIGEST,
    proposal: proposal(),
    aggregateVersion: 1,
    acceptedBy: PRINCIPAL_ID,
    acceptedAt: '2026-08-27T00:00:00.000Z',
  };
}

function acceptedProfileRevision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    workloadProfileId: WORKLOAD_PROFILE_ID,
    workloadProfileRevisionId: WORKLOAD_PROFILE_REVISION_ID,
    revisionNumber: 1,
    buildPlanId: BUILD_PLAN_ID,
    sourceRevisionId: SOURCE_REVISION_ID,
    contractSchema: 'a3s.cloud.workload-profile.v1' as const,
    contractAcl: PROFILE_ACL,
    contractDigest: DIGEST,
    buildPlanDigest: DIGEST,
    projectRoot: '.',
    profile: {
      name: 'web',
      kind: 'web' as const,
      process: {
        command: ['server'],
        args: [],
        workingDirectory: null,
        environment: {},
      },
      secrets: [],
      resources: {
        cpuMillis: 250,
        memoryBytes: 268_435_456,
        pids: 128,
        ephemeralStorageBytes: null,
        executionTimeoutMs: null,
      },
      ports: [{ name: 'http', containerPort: 8080 }],
      health: {
        portName: 'http',
        path: '/health',
        intervalMs: 10_000,
        timeoutMs: 2_000,
        healthyThreshold: 2,
        unhealthyThreshold: 3,
        stabilizationWindowMs: 30_000,
      },
      publicPort: 'http',
      schedule: null,
    },
    acceptedBy: PRINCIPAL_ID,
    acceptedAt: '2026-08-27T00:00:00.000Z',
  };
}

function acceptedPreviewPolicyRevision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    sourceEnvironmentId: ENVIRONMENT_ID,
    sourceSubscriptionId: SOURCE_SUBSCRIPTION_ID,
    pullRequestPreviewPolicyRevisionId: PREVIEW_POLICY_REVISION_ID,
    revisionNumber: 1,
    contractSchema: 'a3s.cloud.pull-request-preview-policy.v1' as const,
    contractAcl: PREVIEW_POLICY_ACL,
    contractDigest: DIGEST,
    policy: previewPolicy(),
    acceptedBy: PRINCIPAL_ID,
    acceptedAt: '2026-08-27T00:00:00.000Z',
  };
}

function previewPolicy() {
  return {
    ownerPrincipalId: PRINCIPAL_ID,
    installationId: 42,
    baseRepository: {
      provider: 'github' as const,
      canonicalUrl: 'https://github.com/a3s-lab/cloud',
    },
    baseBranch: 'main',
    lifetimeSeconds: 86_400,
    maximumActivePreviews: 8,
    forkPolicy: 'isolated' as const,
    allowProtectedSecretsForTrustedSources: true,
    quota: {
      maximumWorkloads: 4,
      cpuMillis: 2_000,
      memoryBytes: 1_073_741_824,
      ephemeralStorageBytes: 1_073_741_824,
    },
  };
}

function pullRequestPreview() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    sourceEnvironmentId: ENVIRONMENT_ID,
    sourceSubscriptionId: SOURCE_SUBSCRIPTION_ID,
    previewId: PREVIEW_ID,
    environmentId: '019c0000-0000-7000-8000-00000000000c',
    environmentName: `pr-42-${PREVIEW_ID.replaceAll('-', '')}`,
    pullRequestId: 42,
    pullRequestNumber: 42,
    policyRevisionId: PREVIEW_POLICY_REVISION_ID,
    policyRevisionNumber: 1,
    policyAcceptedAt: '2026-08-27T00:00:00.000Z',
    policy: previewPolicy(),
    headRepository: {
      provider: 'github' as const,
      canonicalUrl: 'https://github.com/a3s-lab/cloud',
    },
    headBranch: 'feature/preview',
    headCommitSha: COMMIT,
    providerCreatedAt: '2026-08-27T00:00:00.000Z',
    lastProviderUpdatedAt: '2026-08-27T00:01:00.000Z',
    lastChangeKind: 'opened' as const,
    lastMerged: false,
    expiresAt: '2026-08-28T00:01:00.000Z',
    status: 'active' as const,
    cleanupReason: null,
    cleanupRequestedAt: null,
    aggregateVersion: 1,
    isFork: false,
    protectedSecretsEligible: true,
  };
}

function workloadProfileBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workload-profiles`
  );
}

function developerWorkflowEnvironmentBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}`
  );
}

function previewPolicyCollection(): string {
  return `${developerWorkflowEnvironmentBase()}/pull-request-preview-policies`;
}

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

function completeEnvironment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
    A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
  };
}
