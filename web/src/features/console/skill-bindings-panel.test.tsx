import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Asset, AssetRelease, SkillWorkloadRevisionBinding, Workload } from '../../types/api';
import { SkillBindingsPanel } from './skill-bindings-panel';

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

describe('SkillBindingsPanel', () => {
  it('offers only active published Skill bundles and binds one exact release', async () => {
    const onBind = vi.fn().mockResolvedValue(undefined);
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <SkillBindingsPanel
          workload={workload([])}
          assets={[
            asset('skill-1', 'Research tools', 'skill', 'active'),
            asset('agent-1', 'Agent', 'agent', 'active'),
          ]}
          releases={[
            release('release-1', 'skill-1', 'published', 'skill_bundle'),
            release('release-yanked', 'skill-1', 'yanked', 'skill_bundle'),
            release('release-agent', 'agent-1', 'published', 'oci_service'),
          ]}
          onBind={onBind}
          onUnbind={vi.fn()}
        />
      );
    });

    expect([...host.querySelectorAll('option')].map((option) => option.textContent)).toEqual([
      'Research tools',
      '1.0.0 · release-',
    ]);
    const bindButton = [...host.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Bind release')
    );
    if (!bindButton) throw new Error('bind button is missing');
    await act(async () => bindButton.click());
    expect(onBind).toHaveBeenCalledWith('skill-1', 'release-1', expect.stringMatching(/^web-skill-bind:/));
  });

  it('shows the immutable mount and unbinds by Skill Asset identity', async () => {
    const onUnbind = vi.fn().mockResolvedValue(undefined);
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <SkillBindingsPanel
          workload={workload([
            {
              organizationId: 'organization-1',
              assetId: 'skill-1',
              assetReleaseId: 'release-1',
              artifactDigest: `sha256:${'a'.repeat(64)}`,
              artifactMediaType: 'application/vnd.a3s.skill.bundle.v1+tar',
              artifactSizeBytes: 1024,
              mountName: 'skill-skill-1',
              mountTarget: '/a3s/skills/skill-1',
            },
          ])}
          assets={[asset('skill-1', 'Research tools', 'skill', 'active')]}
          releases={[release('release-1', 'skill-1', 'published', 'skill_bundle')]}
          onBind={vi.fn()}
          onUnbind={onUnbind}
        />
      );
    });

    expect(host.textContent).toContain('/a3s/skills/skill-1');
    const unbindButton = [...host.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Unbind')
    );
    if (!unbindButton) throw new Error('unbind button is missing');
    await act(async () => unbindButton.click());
    expect(onUnbind).toHaveBeenCalledWith('skill-1', expect.stringMatching(/^web-skill-unbind:/));
  });

  it('reuses the exact idempotency key when a bind retry follows a failed request', async () => {
    const onBind = vi
      .fn()
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce(undefined);
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <SkillBindingsPanel
          workload={workload([])}
          assets={[asset('skill-1', 'Research tools', 'skill', 'active')]}
          releases={[release('release-1', 'skill-1', 'published', 'skill_bundle')]}
          onBind={onBind}
          onUnbind={vi.fn()}
        />
      );
    });

    const bindButton = [...host.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Bind release')
    );
    if (!bindButton) throw new Error('bind button is missing');
    await act(async () => bindButton.click());
    await act(async () => bindButton.click());

    expect(onBind).toHaveBeenCalledTimes(2);
    expect(onBind.mock.calls[1]?.[2]).toBe(onBind.mock.calls[0]?.[2]);
  });
});

function workload(skillBindings: SkillWorkloadRevisionBinding[]): Workload {
  const revision = {
    id: 'revision-1',
    generation: 1,
    agentBinding: {
      organizationId: 'organization-1',
      assetId: 'agent-1',
      assetReleaseId: 'agent-release-1',
      buildRunId: 'build-1',
    },
    skillBindings,
  } as NonNullable<Workload['desiredRevision']>;
  return {
    id: 'workload-1',
    desiredState: 'running',
    desiredRevision: revision,
    activeRevision: revision,
    deployments: [{ revision, status: 'active' }],
  } as Workload;
}

function asset(id: string, name: string, kind: Asset['kind'], state: Asset['state']): Asset {
  return { id, name, kind, state } as Asset;
}

function release(
  id: string,
  assetId: string,
  state: AssetRelease['state'],
  kind: NonNullable<AssetRelease['artifact']>['kind']
): AssetRelease {
  return {
    id,
    assetId,
    version: '1.0.0',
    state,
    artifact: { kind },
  } as AssetRelease;
}
