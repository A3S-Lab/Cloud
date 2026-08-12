import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { OverviewSection } from './console-sections';

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

describe('OverviewSection', () => {
  it('composes overview surfaces from reusable Card, Item, Empty, and Property List contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <OverviewSection
          operations={[]}
          assets={[]}
          assetReleases={[]}
          buildRunCount={0}
          deployment={undefined}
          routes={[]}
          workloadCount={0}
        />
      );
    });

    expect(host.querySelector('article.card.current-operations-card')).not.toBeNull();
    expect(host.querySelector('.empty.overview-empty-state > figure + header')).not.toBeNull();
    expect(host.querySelector('article.card.authority-chain-card')).not.toBeNull();
    expect(host.querySelectorAll('ol.item-group.authority-chain > li.item')).toHaveLength(7);
    expect(host.querySelector('ol.authority-chain > li.item[data-variant="muted"]')).not.toBeNull();
    expect(host.querySelectorAll('.item-group.asset-kinds > article.item')).toHaveLength(3);
    expect(host.querySelectorAll('dl.property-list.fact-list > div')).toHaveLength(4);
    expect(host.querySelectorAll('dl.property-list.overview-status-facts > div')).toHaveLength(4);
  });
});
