import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Environment, Workload } from '../../types/api';
import { WorkloadList } from './workload-list';

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

describe('WorkloadList', () => {
  it('uses the reusable Card and Empty contracts without workloads', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <WorkloadList workloads={[]} selectedWorkloadId='' environment={undefined} onSelect={vi.fn()} />
      );
    });

    expect(host.querySelector('section.card.workload-section[data-size="sm"]')).not.toBeNull();
    expect(host.querySelector('.badge.card-action[data-variant="secondary"]')).not.toBeNull();
    expect(host.querySelector('.empty.workload-empty > figure + header')).not.toBeNull();
  });

  it('maps selectable workloads to Item and Status Badge contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const onSelect = vi.fn();
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <WorkloadList
          workloads={[workload()]}
          selectedWorkloadId='workload-1'
          environment={environment()}
          onSelect={onSelect}
        />
      );
    });

    const option = host.querySelector<HTMLButtonElement>('[role="listbox"] > button.item[role="option"]');
    expect(option?.getAttribute('aria-selected')).toBe('true');
    expect(option?.getAttribute('data-variant')).toBe('muted');
    expect(option?.querySelector('[data-item-content]')).not.toBeNull();
    expect(option?.querySelector('[data-item-actions]')).not.toBeNull();
    expect(option?.querySelector('.status-badge[data-state="active"][data-indicator]')).not.toBeNull();

    await act(async () => option?.click());
    expect(onSelect).toHaveBeenCalledWith('workload-1');
  });
});

function environment(): Environment {
  return {
    organizationId: 'organization-1',
    projectId: 'project-1',
    id: 'environment-1',
    name: 'Production',
    aggregateVersion: 1,
    createdAt: '2026-08-12T00:00:00Z',
  };
}

function workload(): Workload {
  return {
    id: 'workload-1',
    name: 'coding-agent',
    desiredRevision: {
      generation: 4,
      artifactUri: 'oci://registry.example/coding-agent@sha256:1234',
    },
    deployments: [
      {
        status: 'active',
        operation: { status: 'running' },
        observedRuntime: { state: 'running', healthState: 'healthy' },
      },
    ],
  } as unknown as Workload;
}
