import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { Asset, AssetKind, AssetRelease, AssetReleaseState } from '../../types/api';
import { AssetCatalogCard } from './environment-summary';

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

describe('AssetCatalogCard', () => {
  it('renders authoritative Asset and release lifecycle counts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const assets = [asset('agent-1', 'agent'), asset('agent-2', 'agent'), asset('mcp-1', 'mcp')];
    const releases = [
      release('agent-published', 'agent-1', 'published'),
      release('agent-draft', 'agent-2', 'draft'),
      release('agent-yanked', 'agent-2', 'yanked'),
      release('mcp-published', 'mcp-1', 'published'),
    ];
    root = createRoot(host);

    await act(async () => root?.render(<AssetCatalogCard assets={assets} releases={releases} />));

    expect(host.textContent).toContain('2 assets · 1 published');
    expect(host.textContent).toContain('1 draft · 1 yanked');
    expect(host.textContent).toContain('1 asset · 1 published');
    expect(host.textContent).toContain('0 assets · 0 published');
    expect(host.textContent).toContain('Yanked releases remain available to pinned deployments.');
    expect(host.textContent).not.toContain('No releases');
  });
});

function asset(id: string, kind: AssetKind): Asset {
  return {
    organizationId: 'organization-1',
    id,
    name: id,
    kind,
    state: 'active',
    aggregateVersion: 1,
    createdAt: '2026-08-04T00:00:00.000Z',
    updatedAt: '2026-08-04T00:00:00.000Z',
    archivedAt: null,
  };
}

function release(id: string, assetId: string, state: AssetReleaseState): AssetRelease {
  return {
    organizationId: 'organization-1',
    assetId,
    id,
    version: '1.0.0',
    state,
    commitSha: 'a'.repeat(40),
    manifestDigest: `sha256:${'b'.repeat(64)}`,
    artifact:
      state === 'draft'
        ? null
        : {
            kind: 'oci_service',
            digest: `sha256:${'c'.repeat(64)}`,
            mediaType: 'application/vnd.oci.image.manifest.v1+json',
            sizeBytes: 1024,
          },
    provenance: null,
    aggregateVersion: state === 'draft' ? 1 : state === 'published' ? 2 : 3,
    createdAt: '2026-08-04T00:00:00.000Z',
    updatedAt: '2026-08-04T00:01:00.000Z',
    publishedAt: state === 'draft' ? null : '2026-08-04T00:01:00.000Z',
    yankedAt: state === 'yanked' ? '2026-08-04T00:02:00.000Z' : null,
  };
}
