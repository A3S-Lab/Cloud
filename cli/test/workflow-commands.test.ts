import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_WORKFLOW_GOAL_ACL_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const DEFINITION_ID = '019c0000-0000-7000-8000-000000000003';
const REVISION_ID = '019c0000-0000-7000-8000-000000000004';
const GOAL_ID = '019c0000-0000-7000-8000-000000000005';
const PLAN_ID = '019c0000-0000-7000-8000-000000000006';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000007';
const RUN_ID = '019c0000-0000-7000-8000-000000000008';
const HUMAN_TASK_ID = '019c0000-0000-7000-8000-000000000009';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const DEFINITION_ACL = 'workflow { schema = "cloud.workflow.definition.v1" }\n';
const PAYLOAD_ACL = 'configuration { schema = "cloud.workflow.configuration.v1" }\n';
const GOAL_ACL = 'goal { schema = "cloud.workflow.goal.v1" }\n';
const PUBLICATION = {
  definitionAcl: DEFINITION_ACL,
  payloads: [{ kind: 'configuration', acl: PAYLOAD_ACL }],
};

describe('a3s-cloud Workflow commands', () => {
  it.each([
    [
      ['workflow-definitions', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/workflow-definitions`,
      [definition()],
    ],
    [
      ['workflow-definitions', 'get', DEFINITION_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-definitions/${DEFINITION_ID}`,
      definition(),
    ],
    [
      ['workflow-definitions', 'revisions', DEFINITION_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-definitions/${DEFINITION_ID}/revisions`,
      [revision()],
    ],
    [
      ['workflow-definitions', 'revision', DEFINITION_ID, REVISION_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-definitions/${DEFINITION_ID}/revisions/${REVISION_ID}`,
      revision(),
    ],
    [
      ['workflow-goals', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/workflow-goals`,
      [goal()],
    ],
    [
      ['workflow-goals', 'get', GOAL_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-goals/${GOAL_ID}`,
      goal(),
    ],
    [
      ['workflow-goals', 'plan', GOAL_ID, PLAN_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-goals/${GOAL_ID}/plan-revisions/${PLAN_ID}`,
      planRevision(),
    ],
    [
      ['human-tasks', 'list', 'claimed', '--limit=2'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/human-tasks?status=claimed&limit=2`,
      [humanTask()],
    ],
    [
      ['human-tasks', 'get', HUMAN_TASK_ID],
      `/organizations/${ORGANIZATION_ID}/human-tasks/${HUMAN_TASK_ID}`,
      humanTask(),
    ],
    [
      ['workflow-runs', 'list', '--limit=2'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/workflow-runs?limit=2`,
      [workflowRun()],
    ],
    [
      ['workflow-runs', 'get', RUN_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-runs/${RUN_ID}`,
      workflowRun(),
    ],
    [
      ['workflow-runs', 'wait', RUN_ID, '--wait-seconds=0'],
      `/organizations/${ORGANIZATION_ID}/workflow-runs/${RUN_ID}/wait?timeoutSeconds=0`,
      workflowRun(),
    ],
    [
      ['workflow-runs', 'output', RUN_ID],
      `/organizations/${ORGANIZATION_ID}/workflow-runs/${RUN_ID}/output`,
      workflowRunOutput(),
    ],
    [
      ['workflow-runs', 'history', RUN_ID, '--cursor=7', '--limit=10'],
      `/organizations/${ORGANIZATION_ID}/workflow-runs/${RUN_ID}/history?afterSequence=7&limit=10`,
      workflowRunHistory(),
    ],
  ] as const)('queries the authoritative Workflow lifecycle %#', async (argv, path, data) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli([...argv, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(data);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it('publishes and revises the exact Workflow ACL bundle through JSON transport', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async (path: string) => {
        expect(path).toBe('workflow-publication.json');
        return new TextEncoder().encode(JSON.stringify(PUBLICATION));
      },
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        return envelope(definitionMutation(), 201);
      },
    };
    const created = await runCli(
      [
        'workflow-definitions',
        'create',
        '--file=workflow-publication.json',
        '--idempotency-key=cli:workflow:create',
        '--output=json',
      ],
      runtime
    );
    const revised = await runCli(
      [
        'workflow-definitions',
        'revise',
        DEFINITION_ID,
        '--file=workflow-publication.json',
        '--expected-version=1',
        '--idempotency-key=cli:workflow:revise',
        '--output=json',
      ],
      runtime
    );

    expect(created).toBe(ExitCode.Success);
    expect(revised).toBe(ExitCode.Success);
    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/workflow-definitions`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/workflow-definitions/${DEFINITION_ID}/revisions`,
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(PUBLICATION),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:workflow:create',
        }),
      })
    );
    expect(calls[1]?.[1]?.headers).toEqual(
      expect.objectContaining({
        'Idempotency-Key': 'cli:workflow:revise',
        'x-a3s-expected-version': '1',
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('compiles one WorkflowGoal from bounded ACL through the shared transport', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'workflow-goals',
        'create',
        '--file=goal.acl',
        '--idempotency-key=cli:workflow-goal:create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new TextEncoder().encode(GOAL_ACL),
        fetch: async (...args) => {
          calls.push(args);
          return envelope(goalMutation(), 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/workflow-goals`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        body: GOAL_ACL,
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'cli:workflow-goal:create',
        }),
      })
    );
  });

  it('starts and cancels WorkflowRuns with explicit bounded options and idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        return envelope(workflowRunMutation(), 202);
      },
    };
    const started = await runCli(
      [
        'workflow-runs',
        'start',
        GOAL_ID,
        PLAN_ID,
        '--run-timeout-seconds=60',
        '--idempotency-key=cli:workflow-run:start',
        '--output=json',
      ],
      runtime
    );
    const cancelled = await runCli(
      [
        'workflow-runs',
        'cancel',
        RUN_ID,
        '--reason=operator request',
        '--idempotency-key=cli:workflow-run:cancel',
        '--output=json',
      ],
      runtime
    );

    expect(started).toBe(ExitCode.Success);
    expect(cancelled).toBe(ExitCode.Success);
    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/workflow-runs`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/workflow-runs/${RUN_ID}/cancel`,
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          workflowGoalId: GOAL_ID,
          planRevisionId: PLAN_ID,
          timeoutSeconds: 60,
        }),
        headers: expect.objectContaining({
          'Idempotency-Key': 'cli:workflow-run:start',
        }),
      })
    );
    expect(calls[1]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ reason: 'operator request' }),
        headers: expect.objectContaining({
          'Idempotency-Key': 'cli:workflow-run:cancel',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects out-of-range WorkflowRun options before transport', async () => {
    let called = false;
    const execute = (argv: string[]) => {
      const output = capture();
      return {
        output,
        result: runCli(argv, {
          ...output.runtime,
          environment: completeEnvironment(),
          fetch: async () => {
            called = true;
            return envelope({});
          },
        }),
      };
    };
    const invalidStart = execute([
      'workflow-runs',
      'start',
      GOAL_ID,
      PLAN_ID,
      '--run-timeout-seconds=0',
      '--idempotency-key=cli:workflow-run:start',
    ]);
    const invalidHistory = execute(['workflow-runs', 'history', RUN_ID, '--limit=101']);

    expect(await invalidStart.result).toBe(ExitCode.Usage);
    expect(await invalidHistory.result).toBe(ExitCode.Usage);
    expect(invalidStart.output.stderr()).toContain('WorkflowRun timeout must be between');
    expect(invalidHistory.output.stderr()).toContain('WorkflowRun history limit must be between');
    expect(called).toBe(false);
  });

  it('rejects invalid HumanTask filters before transport', async () => {
    let called = false;
    const execute = (argv: string[]) => {
      const output = capture();
      return {
        output,
        result: runCli(argv, {
          ...output.runtime,
          environment: completeEnvironment(),
          fetch: async () => {
            called = true;
            return envelope({});
          },
        }),
      };
    };
    const invalidStatus = execute(['human-tasks', 'list', 'assigned']);
    const invalidLimit = execute(['human-tasks', 'list', '--limit=201']);
    const invalidDetailOption = execute(['human-tasks', 'get', HUMAN_TASK_ID, '--limit=1']);

    expect(await invalidStatus.result).toBe(ExitCode.Usage);
    expect(await invalidLimit.result).toBe(ExitCode.Usage);
    expect(await invalidDetailOption.result).toBe(ExitCode.Usage);
    expect(invalidStatus.output.stderr()).toContain('HumanTask status is invalid');
    expect(invalidLimit.output.stderr()).toContain('HumanTask list limit must be between');
    expect(invalidDetailOption.output.stderr()).toContain(
      '--limit is valid only for search and log commands'
    );
    expect(called).toBe(false);
  });

  it('rejects malformed publication and oversized goal ACL before transport', async () => {
    let called = false;
    const malformed = capture();
    const malformedExit = await runCli(
      [
        'workflow-definitions',
        'create',
        '--file=workflow-publication.json',
        '--idempotency-key=cli:workflow:invalid',
      ],
      {
        ...malformed.runtime,
        environment: completeEnvironment(),
        readFile: async () => new TextEncoder().encode('{"definitionAcl":"workflow {}","extra":true}'),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );
    const oversized = capture();
    const oversizedExit = await runCli(
      ['workflow-goals', 'create', '--file=goal.acl', '--idempotency-key=cli:workflow-goal:oversized'],
      {
        ...oversized.runtime,
        environment: completeEnvironment(),
        readFile: async () => new Uint8Array(MAX_WORKFLOW_GOAL_ACL_BYTES + 1),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );

    expect(malformedExit).toBe(ExitCode.Usage);
    expect(oversizedExit).toBe(ExitCode.Usage);
    expect(malformed.stderr()).toContain('only definitionAcl and typed ACL payloads');
    expect(oversized.stderr()).toContain('Workflow goal ACL must contain between');
    expect(called).toBe(false);
  });
});

function definition() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: DEFINITION_ID,
    name: 'Support triage',
    description: 'Exact Workflow',
    currentRevisionId: REVISION_ID,
    currentRevisionNumber: 1,
    currentRevisionDigest: DIGEST,
    aggregateVersion: 1,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-07T00:00:00.000Z',
    updatedAt: '2026-08-07T00:00:00.000Z',
  };
}

function revision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    workflowDefinitionId: DEFINITION_ID,
    id: REVISION_ID,
    revisionNumber: 1,
    parentRevisionId: null,
    parentDigest: null,
    contractSchema: 'cloud.workflow.definition.v1',
    compilerSchemaVersion: 1,
    contentDigest: DIGEST,
    payloadSetDigest: DIGEST,
    payloadCount: 1,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-07T00:00:00.000Z',
    canonicalDefinitionAcl: DEFINITION_ACL,
    payloads: [
      {
        kind: 'configuration',
        schema: 'cloud.workflow.configuration.v1',
        digest: DIGEST,
        canonicalAcl: PAYLOAD_ACL,
      },
    ],
  };
}

function definitionMutation() {
  return { workflowDefinition: definition(), revision: revision(), replayed: false };
}

function goal() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: GOAL_ID,
    name: 'Resolve support case',
    contractSchema: 'cloud.workflow.goal.v1',
    contractDigest: DIGEST,
    inputDigest: DIGEST,
    canonicalGoalAcl: GOAL_ACL,
    workflowDefinitionId: DEFINITION_ID,
    workflowRevisionId: REVISION_ID,
    workflowDigest: DIGEST,
    ontologyId: ORGANIZATION_ID,
    ontologyRevisionId: PROJECT_ID,
    ontologyDigest: DIGEST,
    environmentId: null,
    input: { caseId: 'T-42' },
    planRevisionId: PLAN_ID,
    planDigest: DIGEST,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-07T00:00:00.000Z',
  };
}

function planRevision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    workflowGoalId: GOAL_ID,
    id: PLAN_ID,
    schema: 'cloud.workflow.plan.v1',
    compilerRevision: 'cloud.workflow.plan-compiler.v1',
    digest: DIGEST,
    canonicalPlan: '{}',
    plan: {
      schema: 'cloud.workflow.plan.v1',
      compilerRevision: 'cloud.workflow.plan-compiler.v1',
      workflowDefinitionId: DEFINITION_ID,
      workflowRevisionId: REVISION_ID,
      workflowDigest: DIGEST,
      workflowPayloadSetDigest: DIGEST,
      ontologyId: ORGANIZATION_ID,
      ontologyRevisionId: PROJECT_ID,
      ontologyDigest: DIGEST,
      environmentId: null,
      inputDigest: DIGEST,
      steps: [],
      edges: [],
    },
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-07T00:00:00.000Z',
  };
}

function goalMutation() {
  return { goal: goal(), planRevision: planRevision(), replayed: false };
}

function workflowRun() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: RUN_ID,
    workflowGoalId: GOAL_ID,
    planRevisionId: PLAN_ID,
    planDigest: DIGEST,
    operationId: RUN_ID,
    flowRunId: RUN_ID,
    flowRuntimeBuildId: null,
    executionInputDigest: DIGEST,
    status: 'pending',
    lastFlowSequence: 0,
    outputDigest: null,
    error: null,
    aggregateVersion: 1,
    requestedBy: PRINCIPAL_ID,
    requestedAt: '2026-08-09T00:00:00.000Z',
    updatedAt: '2026-08-09T00:00:00.000Z',
    startedAt: null,
    deadlineAt: '2026-08-09T01:00:00.000Z',
    cancellationRequestedAt: null,
    cancellationReason: null,
    finishedAt: null,
    steps: [
      {
        stepId: 'input',
        kind: 'input',
        status: 'pending',
        flowStepId: 'workflow:input',
        attemptGeneration: 0,
        selectedHandle: null,
        result: null,
        resultDigest: null,
        error: null,
        evidenceReferences: [],
        lastFlowSequence: 0,
        updatedAt: '2026-08-09T00:00:00.000Z',
      },
    ],
  };
}

function workflowRunMutation() {
  return { workflowRun: workflowRun(), replayed: false };
}

function workflowRunOutput() {
  return {
    workflowRunId: RUN_ID,
    output: { result: 'done' },
    outputDigest: DIGEST,
    finishedAt: '2026-08-09T00:01:00.000Z',
  };
}

function workflowRunHistory() {
  return {
    events: [
      {
        sequence: 8,
        eventId: '019c0000-0000-7000-8000-000000000009',
        eventKey: 'flow.step.completed',
        occurredAt: '2026-08-09T00:00:30.000Z',
        stepId: 'input',
        attempt: 1,
        details: { result: 'redacted' },
      },
    ],
    nextSequence: null,
  };
}

function humanTask() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: HUMAN_TASK_ID,
    workflowRunId: RUN_ID,
    stepId: 'human_review',
    stepAttempt: 1,
    formRelease: {
      apiVersion: 'a3s.dev/form-release-ref/v1',
      organizationId: ORGANIZATION_ID,
      projectId: PROJECT_ID,
      formId: DEFINITION_ID,
      releaseId: REVISION_ID,
      uri: `a3s://forms/${DEFINITION_ID}/releases/${REVISION_ID}`,
      revision: 1,
      digest: DIGEST,
      compilerRevision: 'a3s-form-core@0.1.0',
      schemaProfile: 'a3s.dev/form-schema-profile/1',
      mode: 'interaction',
    },
    assignmentPolicy: {
      id: 'cloud.workflow.assignment.organization-member-exclusive',
      revision: 1,
      digest: DIGEST,
    },
    status: 'claimed',
    claimedBy: PRINCIPAL_ID,
    decisionId: null,
    aggregateVersion: 3,
    message: 'Review this change',
    allowedOutcomes: ['approve', 'reject'],
    createdAt: '2026-08-09T00:00:00.000Z',
    updatedAt: '2026-08-09T00:01:00.000Z',
    dueAt: null,
    expiresAt: '2026-08-09T01:00:00.000Z',
    claimedAt: '2026-08-09T00:01:00.000Z',
    terminalAt: null,
    details: null,
    outputMapping: { kind: 'identity' },
    maxValueBytes: 4096,
    initialValue: null,
    interactionRequest: null,
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-07T00:00:00.000Z',
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
  };
}
