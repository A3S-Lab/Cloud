import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Operation } from '../../types/api';
import { OperationDrawer } from './operation-drawer';

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

describe('OperationDrawer', () => {
  it('uses Status Badge and Empty contracts while the stream is live', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <OperationDrawer
          operations={[]}
          dismissedOperationIds={new Set()}
          streamState='live'
          onDismissTerminal={vi.fn()}
        />
      );
    });

    expect(host.querySelector('.drawer-heading > .status-badge[data-state="active"]')).not.toBeNull();
    expect(host.querySelector('.empty.empty-operations > figure + header')).not.toBeNull();
    expect(host.querySelector('aside.task-pane.operation-drawer[data-responsive="overlay"]')).not.toBeNull();
    expect(host.querySelector('aside.task-pane > section.operation-list')).not.toBeNull();
  });

  it('maps durable operations to Item, Status Badge, and Button contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const onDismissTerminal = vi.fn();
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <OperationDrawer
          operations={[operation()]}
          dismissedOperationIds={new Set()}
          streamState='retrying'
          onDismissTerminal={onDismissTerminal}
        />
      );
    });

    expect(host.querySelector('.drawer-heading > .status-badge[data-state="warning"]')).not.toBeNull();
    expect(
      host.querySelector('.item-group.operation-list > article.item[data-variant="outline"]')
    ).not.toBeNull();
    expect(host.querySelector('.operation-item .status-badge[data-state="success"]')).not.toBeNull();
    const clear = host.querySelector<HTMLButtonElement>('button.btn.drawer-cleanup[data-size="xs"]');
    expect(clear).not.toBeNull();

    await act(async () => clear?.click());
    expect(onDismissTerminal).toHaveBeenCalledWith(['operation-1']);
  });
});

function operation(): Operation {
  return {
    id: 'operation-1',
    organizationId: 'organization-1',
    subjectKind: 'workload',
    subjectId: 'workload-1',
    workflowName: 'deploy-workload',
    workflowVersion: '1',
    status: 'succeeded',
    lastSequence: 7,
    requestedAt: '2026-08-12T00:00:00Z',
    updatedAt: '2026-08-12T00:01:00Z',
    error: null,
  };
}
