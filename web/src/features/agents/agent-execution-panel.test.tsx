import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CloudApi } from '../../lib/api';
import {
  AgentExecutionPanel,
  agentEventStatusState,
  agentExecutionStatusState,
  agentStreamStatusState,
} from './agent-execution-panel';

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

describe('AgentExecutionPanel', () => {
  it('projects the provider-neutral Agent Workbench regions', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <AgentExecutionPanel
          api={new CloudApi('token')}
          organizationId={null}
          projectId={null}
          environmentId={null}
          conversations={[]}
          selectedConversationId=''
          assets={[]}
          releases={[]}
          onSelectConversation={vi.fn()}
          onConversationChanged={vi.fn()}
          onError={vi.fn()}
        />
      );
    });

    const workbench = host.querySelector('.agent-workbench');
    expect(workbench?.getAttribute('aria-label')).toBe('Agent execution workbench');
    expect(workbench?.querySelector(':scope > [data-agent-context]')).not.toBeNull();
    expect(workbench?.querySelector(':scope > [data-agent-canvas]')).not.toBeNull();
    expect(workbench?.querySelector(':scope > [data-agent-activity]')).not.toBeNull();
    expect(workbench?.querySelectorAll(':scope > article.card')).toHaveLength(3);
    expect(
      workbench?.querySelector('.field > label[for="agent-release-binding"] + select.select')
    ).not.toBeNull();
    expect(workbench?.querySelectorAll('button.btn')).toHaveLength(2);
    expect(workbench?.querySelectorAll('.empty')).toHaveLength(3);
    expect(workbench?.querySelector('ol.timeline[reversed]')).not.toBeNull();
    expect(workbench?.querySelector('.status-badge[data-state="neutral"][data-indicator]')).not.toBeNull();
  });

  it('maps execution, stream, and event states onto the Status Badge contract', () => {
    expect(
      (['pending', 'running', 'cancelling', 'succeeded', 'failed', 'cancelled'] as const).map(
        agentExecutionStatusState
      )
    ).toEqual(['neutral', 'active', 'warning', 'success', 'danger', 'danger']);
    expect((['idle', 'connecting', 'live', 'retrying'] as const).map(agentStreamStatusState)).toEqual([
      'neutral',
      'warning',
      'active',
      'warning',
    ]);
    expect(
      (
        [
          'execution_requested',
          'model_output',
          'execution_failed',
          'execution_completed',
          'execution_cancelled',
        ] as const
      ).map(agentEventStatusState)
    ).toEqual(['neutral', 'active', 'danger', 'success', 'danger']);
  });
});
