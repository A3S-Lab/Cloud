import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Deployment, ServiceTemplate, Workload, WorkloadRevision } from '../../types/api';
import { WorkloadActions } from './workload-actions';

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
  vi.restoreAllMocks();
});

describe('WorkloadActions', () => {
  it('traps reverse focus from dialog entry and restores focus on escape', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);
    await act(async () => {
      root?.render(<WorkloadActions workload={workload()} onUpdate={vi.fn()} onRollback={vi.fn()} />);
    });

    const updateButton = [...host.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Update')
    );
    if (!updateButton) throw new Error('update button is missing');
    updateButton.focus();
    await act(async () => updateButton.click());

    const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
    const editor = dialog?.querySelector('textarea');
    expect(dialog).not.toBeNull();
    expect(document.activeElement).toBe(dialog);
    expect(host.hasAttribute('inert')).toBe(true);

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }));
    expect(document.activeElement).toBe(editor);

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(host.hasAttribute('inert')).toBe(false);
    expect(document.activeElement).toBe(updateButton);
  });
});

function workload(): Workload {
  const current = revision();
  return {
    id: 'workload-1',
    organizationId: 'organization-1',
    projectId: 'project-1',
    environmentId: 'environment-1',
    name: 'api',
    desiredState: 'running',
    desiredRevision: current,
    activeRevision: current,
    deployments: [deployment(current)],
    aggregateVersion: 1,
    createdAt: current.createdAt,
    updatedAt: current.createdAt,
  };
}

function revision(): WorkloadRevision {
  return {
    id: 'revision-1',
    generation: 1,
    requestedTemplate: template,
    artifactSourceUri: template.artifact.uri,
    expectedArtifactDigest: null,
    requestDigest: 'sha256:request',
    artifactUri: 'oci://registry.example/cloud/api@sha256:aaaaaaaa',
    artifactDigest: 'sha256:aaaaaaaa',
    artifactMediaType: 'application/vnd.oci.image.manifest.v1+json',
    templateDigest: 'sha256:template',
    createdAt: '2026-07-20T00:00:00Z',
    resolvedAt: '2026-07-20T00:00:01Z',
  };
}

function deployment(workloadRevision: WorkloadRevision): Deployment {
  return {
    id: 'deployment-1',
    workloadId: 'workload-1',
    revision: workloadRevision,
    operationId: 'operation-1',
    nodeId: 'node-1',
    commandId: 'command-1',
    cleanupCommandId: null,
    retirementCommandId: null,
    status: 'active',
    failure: null,
    operation: null,
    observedRuntime: null,
    aggregateVersion: 1,
    requestedAt: workloadRevision.createdAt,
    updatedAt: workloadRevision.createdAt,
    activatedAt: workloadRevision.createdAt,
    cancellationRequestedAt: null,
    cancelledAt: null,
  };
}

const template: ServiceTemplate = {
  artifact: { uri: 'oci://registry.example/cloud/api:v1', expectedDigest: null },
  process: { command: ['/service'], args: [], workingDirectory: '/app', environment: {} },
  secrets: [],
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
