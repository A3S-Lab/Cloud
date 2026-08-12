import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { GatewayCertificate, Route, Workload } from '../../types/api';
import { EdgeStatusPanel } from './edge-status-panel';

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

describe('EdgeStatusPanel', () => {
  it('composes route and certificate projections from reusable contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <EdgeStatusPanel workload={workload()} routes={[route()]} certificates={[certificate()]} />
      );
    });

    expect(host.querySelector('section.card.edge-status-panel')).not.toBeNull();
    expect(host.querySelector('.badge.panel-count')?.textContent?.trim()).toBe('1');
    expect(host.querySelector('.item-group > article.item.edge-route')).not.toBeNull();
    expect(host.querySelector('dl.property-list.edge-facts[data-size="sm"]')).not.toBeNull();
    expect(host.querySelector('article.item.certificate-projection')).not.toBeNull();
    expect(
      [...host.querySelectorAll('.status-badge')].map((badge) => badge.getAttribute('data-state'))
    ).toEqual(['active', 'success']);
  });

  it('uses the reusable Empty contract when no route is projected', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(<EdgeStatusPanel workload={undefined} routes={[]} certificates={[]} />);
    });

    expect(host.querySelector('.empty.detail-empty > figure + header')).not.toBeNull();
    expect(host.querySelector('.item-group')).toBeNull();
  });
});

function workload(): Workload {
  return {
    deployments: [{ revision: { id: 'revision-7', generation: 7 } }],
  } as unknown as Workload;
}

function route(): Route {
  return {
    id: 'route-1',
    gatewayCertificateId: 'certificate-1',
    workloadRevisionId: 'revision-7',
    hostname: 'agent.example.test',
    pathPrefix: '/run',
    state: 'active',
    gatewayNodeId: 'node-12345678',
    gatewayRevision: 4,
    activatedAt: '2026-08-12T00:00:00Z',
    snapshotDigest: `sha256:${'a'.repeat(64)}`,
    failure: null,
  } as Route;
}

function certificate(): GatewayCertificate {
  return {
    id: 'certificate-1',
    dnsNames: ['agent.example.test'],
    state: 'ready',
    fingerprint: `sha256:${'b'.repeat(64)}`,
    expiresAt: '2027-08-12T00:00:00Z',
    failure: null,
  } as GatewayCertificate;
}
