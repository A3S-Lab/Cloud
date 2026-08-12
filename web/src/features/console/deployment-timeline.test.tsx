import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { Deployment, ServiceTemplate, Workload, WorkloadRevision } from '../../types/api';
import { DeploymentTimeline } from './deployment-timeline';

let root: Root | null = null;

beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
});

describe('DeploymentTimeline', () => {
  it('maps the current deployment to the reusable Timeline and Property List contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(<DeploymentTimeline workload={workload()} operations={[]} />);
    });

    const timeline = host.querySelector('ol.timeline');
    const current = timeline?.querySelector(':scope > li');
    expect(host.querySelector('section.card.deployment-timeline')).not.toBeNull();
    expect(host.querySelector('.badge.panel-count')?.textContent?.trim()).toBe('1');
    expect(current?.getAttribute('data-marker')).toBe('7');
    expect(current?.getAttribute('data-state')).toBe('active');
    expect(current?.getAttribute('aria-current')).toBe('step');
    expect(current?.querySelector('dl.property-list[data-size="sm"]')).not.toBeNull();
    expect(current?.querySelector('.status-badge[data-state="active"][data-indicator]')).not.toBeNull();
    expect(current?.querySelector('article.item[data-variant="outline"]')).not.toBeNull();
  });

  it('uses the reusable Empty contract without deployment history', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(<DeploymentTimeline workload={undefined} operations={[]} />);
    });

    expect(host.querySelector('.empty.detail-empty > figure + header')).not.toBeNull();
    expect(host.querySelector('ol.timeline')).toBeNull();
  });
});

function workload(): Workload {
  const template: ServiceTemplate = {
    artifact: { uri: 'oci://registry.example/workload:latest', expectedDigest: null },
    process: { command: ['/app'], args: [], workingDirectory: null, environment: {} },
    secrets: [],
    resources: {
      cpuMillis: 100,
      memoryBytes: 128_000_000,
      pids: 32,
      ephemeralStorageBytes: null,
    },
    ports: [],
    health: {
      portName: 'http',
      path: '/health',
      intervalMs: 1_000,
      timeoutMs: 500,
      healthyThreshold: 1,
      unhealthyThreshold: 3,
      stabilizationWindowMs: 5_000,
    },
  };
  const revision: WorkloadRevision = {
    id: 'revision-7',
    generation: 7,
    requestedTemplate: template,
    artifactUri: 'oci://registry.example/workload@sha256:1234',
    artifactSourceUri: template.artifact.uri,
    expectedArtifactDigest: null,
    requestDigest: 'sha256:request',
    artifactDigest: 'sha256:1234',
    artifactMediaType: 'application/vnd.oci.image.manifest.v1+json',
    templateDigest: 'sha256:template',
    createdAt: '2026-08-12T00:00:00Z',
    resolvedAt: '2026-08-12T00:00:01Z',
    skillBindings: [],
  };
  const deployment: Deployment = {
    id: 'deployment-7',
    workloadId: 'workload-1',
    replicaId: 'replica-1',
    replicaGeneration: 1,
    memberId: 'member-1',
    placementGeneration: 1,
    revision,
    operationId: 'operation-7',
    nodeId: 'node-1',
    runtimeUnitId: 'runtime-1',
    runtimeGeneration: 1,
    commandId: 'command-1',
    cleanupCommandId: null,
    retirementCommandId: null,
    status: 'active',
    failure: null,
    operation: null,
    observedRuntime: null,
    aggregateVersion: 1,
    requestedAt: '2026-08-12T00:00:00Z',
    updatedAt: '2026-08-12T00:01:00Z',
    activatedAt: '2026-08-12T00:01:00Z',
    cancellationRequestedAt: null,
    cancelledAt: null,
  };
  return {
    id: 'workload-1',
    organizationId: 'organization-1',
    projectId: 'project-1',
    environmentId: 'environment-1',
    name: 'workload',
    desiredState: 'running',
    control: {
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
      createdAt: '2026-08-12T00:00:00Z',
      updatedAt: '2026-08-12T00:00:00Z',
    },
    replicas: [],
    desiredRevision: revision,
    activeRevision: revision,
    deployments: [deployment],
    aggregateVersion: 1,
    createdAt: '2026-08-12T00:00:00Z',
    updatedAt: '2026-08-12T00:01:00Z',
  };
}
