import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Workload } from '../../types/api';
import { WorkloadOverview } from './workload-overview';

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

describe('WorkloadOverview', () => {
  it('uses Card and neutral Status Badge contracts before a workload is selected', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <WorkloadOverview
          workload={undefined}
          routes={[]}
          cancelling={false}
          stopping={false}
          onCancel={vi.fn()}
          onStop={vi.fn()}
          onUpdate={vi.fn()}
          onRollback={vi.fn()}
        />
      );
    });

    expect(host.querySelector('article.card.surface.convergence-card[data-size="sm"]')).not.toBeNull();
    expect(host.querySelector('.status-badge[data-state="neutral"][data-indicator]')).not.toBeNull();
    expect(host.querySelector('.surface-note')).not.toBeNull();
  });

  it('projects deployment facts through Property List', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <WorkloadOverview
          workload={workload()}
          routes={[]}
          cancelling={false}
          stopping={false}
          onCancel={vi.fn()}
          onStop={vi.fn()}
          onUpdate={vi.fn()}
          onRollback={vi.fn()}
        />
      );
    });

    expect(host.querySelector('.status-badge[data-state="active"]')).not.toBeNull();
    expect(host.querySelectorAll('dl.property-list.deployment-facts > div')).toHaveLength(5);
    expect(host.querySelectorAll('ol.stepper.convergence-track > li')).toHaveLength(4);
    expect(host.querySelector<HTMLOListElement>('ol.stepper.convergence-track')?.tabIndex).toBe(0);
    expect(host.querySelector('ol.stepper [data-state="success"] [data-step-marker]')).not.toBeNull();
    expect(host.querySelector('ol.stepper [data-state="active"][aria-current="step"]')).not.toBeNull();
  });
});

function workload(): Workload {
  return {
    id: 'workload-1',
    name: 'coding-agent',
    desiredState: 'running',
    desiredRevision: null,
    activeRevision: null,
    deployments: [
      {
        status: 'verifying',
        revision: { id: 'revision-1' },
        observedRuntime: null,
      },
    ],
  } as unknown as Workload;
}
