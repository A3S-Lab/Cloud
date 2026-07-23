import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BuildRun, BuildRunStatus } from '../../types/api';
import { BuildRunPanel } from './build-run-panel';

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

describe('BuildRunPanel', () => {
  it('shows authoritative status and exposes cancellation only for nonterminal builds', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const onCancel = vi.fn();
    const onRetry = vi.fn();
    const onSelect = vi.fn();
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <BuildRunPanel
          buildRuns={[
            buildRun('running'),
            buildRun('succeeded', 'build-succeeded'),
            buildRun('failed', 'build-failed'),
          ]}
          selectedBuildRunId='build-running'
          cancellingBuildRunId={null}
          retryingBuildRunId={null}
          onSelect={onSelect}
          onCancel={onCancel}
          onRetry={onRetry}
        />
      );
    });

    expect(host.textContent).toContain('Running');
    expect(host.textContent).toContain('Succeeded');
    expect(host.textContent).toContain('Attempt 1');
    const cancelButtons = [...host.querySelectorAll('button')].filter((button) =>
      button.textContent?.includes('Cancel build')
    );
    expect(cancelButtons).toHaveLength(1);
    await act(async () => cancelButtons[0]?.click());
    expect(onCancel).toHaveBeenCalledWith('build-running');
    const retryButtons = [...host.querySelectorAll('button')].filter((button) =>
      button.textContent?.includes('Retry build')
    );
    expect(retryButtons).toHaveLength(1);
    await act(async () => retryButtons[0]?.click());
    expect(onRetry).toHaveBeenCalledWith('build-failed');
    const viewButtons = [...host.querySelectorAll('button')].filter((button) =>
      button.textContent?.includes('View logs')
    );
    expect(viewButtons).toHaveLength(2);
    await act(async () => viewButtons[0]?.click());
    expect(onSelect).toHaveBeenCalledWith('build-succeeded');
  });
});

function buildRun(status: BuildRunStatus, id = `build-${status}`): BuildRun {
  return {
    organizationId: 'organization-1',
    projectId: 'project-1',
    environmentId: 'environment-1',
    id,
    sourceRevisionId: `source-${status}`,
    attempt: 1,
    retryOfBuildRunId: null,
    operationId: `operation-${status}`,
    status,
    sourceContentDigest: `sha256:${'a'.repeat(64)}`,
    output: null,
    publicationTarget: null,
    publishedArtifact: null,
    failure: null,
    aggregateVersion: 2,
    requestedAt: '2026-07-22T00:00:00Z',
    updatedAt: '2026-07-22T00:01:00Z',
    startedAt: '2026-07-22T00:00:01Z',
    cancellationRequestedAt: null,
    finishedAt: status === 'succeeded' ? '2026-07-22T00:01:00Z' : null,
  };
}
