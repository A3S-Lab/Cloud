import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { BuildRun } from '../../types/api';
import { BuildRunLogPanel } from './build-run-log-panel';

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

describe('BuildRunLogPanel', () => {
  it('shows the authoritative Box-log unavailability without stream controls', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(<BuildRunLogPanel buildRun={buildRun()} />);
    });

    expect(host.textContent).toContain('A3S Box contract pending');
    expect(host.textContent).toContain(
      'Build logs are unavailable until A3S Box exposes an authoritative durable log contract.'
    );
    expect([...host.querySelectorAll('button')].every((button) => button.disabled)).toBe(true);
  });

  it('asks for a selection without claiming that logs are available', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(<BuildRunLogPanel buildRun={null} />);
    });

    expect(host.textContent).toContain('Select a build run to inspect log availability.');
    expect(host.textContent).not.toContain('Connecting to the authoritative log stream.');
  });
});

function buildRun(): BuildRun {
  return {
    organizationId: 'organization-1',
    projectId: 'project-1',
    environmentId: 'environment-1',
    id: 'build-1',
    sourceRevisionId: 'source-1',
    attempt: 1,
    retryOfBuildRunId: null,
    operationId: 'operation-1',
    status: 'running',
    sourceContentDigest: `sha256:${'a'.repeat(64)}`,
    output: null,
    publicationTarget: null,
    publishedArtifact: null,
    evidenceSummary: null,
    failure: null,
    aggregateVersion: 1,
    requestedAt: '2026-07-31T00:00:00Z',
    updatedAt: '2026-07-31T00:01:00Z',
    startedAt: '2026-07-31T00:00:01Z',
    cancellationRequestedAt: null,
    finishedAt: null,
  };
}
