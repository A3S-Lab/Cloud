import { describe, expect, it } from 'vitest';
import type {
  Deployment,
  Operation,
  Route,
  ServiceTemplate,
  Workload,
  WorkloadRevision,
} from '../../types/api';
import {
  diffTemplates,
  eligibleRollbackRevisions,
  isWorkloadReadyForReplacement,
  parseServiceTemplateDraft,
  routeStage,
  visibleOperations,
} from './workload-view-model';

const template: ServiceTemplate = {
  artifact: {
    uri: 'oci://registry.example/cloud/api:v1',
    expectedDigest: null,
  },
  process: {
    command: [],
    args: [],
    workingDirectory: null,
    environment: {},
  },
  secrets: [
    {
      name: 'database-url',
      secretId: 'secret-1',
      version: 1,
      target: { kind: 'environment', variable: 'DATABASE_URL' },
    },
  ],
  resources: {
    cpuMillis: 100,
    memoryBytes: 33_554_432,
    pids: 32,
    ephemeralStorageBytes: null,
  },
  ports: [{ name: 'http', containerPort: 8080 }],
  health: {
    portName: 'http',
    path: '/health',
    intervalMs: 1_000,
    timeoutMs: 500,
    healthyThreshold: 1,
    unhealthyThreshold: 3,
    stabilizationWindowMs: 1_000,
  },
};

describe('workload view model', () => {
  it('produces stable field-level changes for a complete immutable template', () => {
    const candidate = structuredClone(template);
    candidate.artifact.uri = 'oci://registry.example/cloud/api:v2';
    candidate.secrets[0].version = 2;
    candidate.process.args = ['serve'];

    expect(diffTemplates(template, candidate)).toEqual([
      {
        path: 'artifact.uri',
        before: 'oci://registry.example/cloud/api:v1',
        after: 'oci://registry.example/cloud/api:v2',
      },
      {
        path: 'process.args[0]',
        before: 'Not set',
        after: 'serve',
      },
      {
        path: 'secrets[0].version',
        before: '1',
        after: '2',
      },
    ]);
  });

  it('selects only older successfully activated revisions from the same running workload', () => {
    const generationOne = revision('revision-1', 1);
    const failedGeneration = revision('revision-2', 2);
    const current = revision('revision-3', 3);
    const workload: Workload = {
      id: 'workload-1',
      organizationId: 'organization-1',
      projectId: 'project-1',
      environmentId: 'environment-1',
      name: 'api',
      desiredState: 'running',
      control: workloadControl(current.createdAt),
      replicas: [],
      desiredRevision: current,
      activeRevision: current,
      deployments: [
        deployment('deployment-3', current, 'active', true),
        deployment('deployment-2', failedGeneration, 'failed', false),
        deployment('deployment-1', generationOne, 'active', true),
      ],
      aggregateVersion: 3,
      createdAt: '2026-07-20T00:00:00Z',
      updatedAt: '2026-07-20T00:00:03Z',
    };

    expect(eligibleRollbackRevisions(workload).map((item) => item.id)).toEqual(['revision-1']);
    expect(
      eligibleRollbackRevisions({
        ...workload,
        desiredState: 'stopped',
      })
    ).toEqual([]);
    expect(isWorkloadReadyForReplacement(workload)).toBe(true);
    expect(
      isWorkloadReadyForReplacement({
        ...workload,
        desiredRevision: failedGeneration,
      })
    ).toBe(false);
  });

  it('accepts a complete template draft and rejects an incomplete shape', () => {
    expect(parseServiceTemplateDraft(JSON.stringify(template))).toEqual({
      template,
      error: null,
    });
    expect(parseServiceTemplateDraft('{"artifact":{}}')).toEqual({
      template: null,
      error: 'The template must include artifact, process, secrets, resources, ports, and health.',
    });
    expect(parseServiceTemplateDraft('{')).toEqual({
      template: null,
      error: 'The template is not valid JSON.',
    });
  });

  it('hides only explicitly dismissed terminal operations and preserves active work', () => {
    const operations = [
      operation('running', 'running'),
      operation('succeeded', 'succeeded'),
      operation('failed', 'failed'),
    ];

    expect(visibleOperations(operations, new Set(['succeeded', 'running'])).map((item) => item.id)).toEqual([
      'running',
      'failed',
    ]);
  });

  it('surfaces an unavailable current route as failed convergence', () => {
    expect(routeStage('revision-3', [route('unavailable')])).toEqual({
      name: 'Route active',
      label: 'Unavailable',
      state: 'failed',
    });
  });
});

function revision(id: string, generation: number): WorkloadRevision {
  return {
    id,
    generation,
    requestedTemplate: structuredClone(template),
    artifactSourceUri: template.artifact.uri,
    expectedArtifactDigest: null,
    requestDigest: `request-${generation}`,
    artifactUri: `oci://registry.example/cloud/api@sha256:${String(generation).repeat(64)}`,
    artifactDigest: `sha256:${String(generation).repeat(64)}`,
    artifactMediaType: 'application/vnd.oci.image.manifest.v1+json',
    templateDigest: `template-${generation}`,
    createdAt: `2026-07-20T00:00:0${generation}Z`,
    resolvedAt: `2026-07-20T00:00:0${generation}Z`,
    skillBindings: [],
  };
}

function deployment(
  id: string,
  workloadRevision: WorkloadRevision,
  status: Deployment['status'],
  activated: boolean
): Deployment {
  return {
    id,
    workloadId: 'workload-1',
    replicaId: `replica-${id}`,
    replicaGeneration: workloadRevision.generation,
    memberId: `member-${id}`,
    placementGeneration: 1,
    revision: workloadRevision,
    operationId: `operation-${id}`,
    nodeId: 'node-1',
    runtimeUnitId: `runtime-${id}`,
    runtimeGeneration: workloadRevision.generation,
    commandId: 'command-1',
    cleanupCommandId: null,
    retirementCommandId: null,
    status,
    failure: status === 'failed' ? 'health failed' : null,
    operation: null,
    observedRuntime: null,
    aggregateVersion: 1,
    requestedAt: workloadRevision.createdAt,
    updatedAt: workloadRevision.createdAt,
    activatedAt: activated ? workloadRevision.createdAt : null,
    cancellationRequestedAt: null,
    cancelledAt: null,
  };
}

function workloadControl(timestamp: string): Workload['control'] {
  return {
    managedOwner: null,
    placementPolicy: {
      schema: 'a3s.cloud.effective-placement-policy.v2',
      generation: 1,
      desiredReplicas: 1,
      membersPerReplica: 1,
      topology: 'single_node',
      replicaAntiAffinity: 'required',
      digest: 'sha256:placement',
    },
    aggregateVersion: 1,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

function operation(id: string, status: Operation['status']): Operation {
  return {
    id,
    organizationId: 'organization-1',
    subjectKind: 'deployment',
    subjectId: `deployment-${id}`,
    workflowName: 'cloud.deployment',
    workflowVersion: '2',
    status,
    lastSequence: 3,
    requestedAt: '2026-07-20T00:00:00Z',
    updatedAt: '2026-07-20T00:00:01Z',
    error: null,
  };
}

function route(state: Route['state']): Route {
  return {
    id: 'route-1',
    organizationId: 'organization-1',
    projectId: 'project-1',
    environmentId: 'environment-1',
    gatewayScopeId: 'gateway-scope-1',
    gatewayNodeId: 'gateway-node-1',
    hostname: 'api.example.test',
    pathPrefix: '/',
    domainClaimId: null,
    domainPattern: null,
    gatewayCertificateId: null,
    workloadId: 'workload-1',
    workloadRevisionId: 'revision-3',
    runtimeUnitId: 'runtime-unit-1',
    runtimeGeneration: 3,
    portName: 'http',
    upstreamOrigin: 'http://10.0.0.3:8080',
    targetObservedAt: '2026-07-20T00:00:03Z',
    state,
    gatewayRevision: null,
    gatewayCommandId: null,
    snapshotDigest: null,
    failure: state === 'unavailable' ? 'Gateway rollout is below readiness threshold' : null,
    aggregateVersion: 1,
    createdAt: '2026-07-20T00:00:03Z',
    updatedAt: '2026-07-20T00:00:03Z',
    activatedAt: null,
  };
}
