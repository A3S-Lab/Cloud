import { describe, expect, it } from 'bun:test';
import {
  type BuildPlanDetection,
  type CloudFetch,
  MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES,
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
const DIGEST = `sha256:${'a'.repeat(64)}`;
const COMMIT = 'b'.repeat(40);
const PROPOSAL_ACL = 'build_plan { schema = "a3s.cloud.build-plan-proposal.v1" detector = "dockerfile" }\n';
const PROFILE_ACL = 'workload_profile { schema = "a3s.cloud.workload-profile.v1" }\n';

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

function workloadProfileBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workload-profiles`
  );
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
