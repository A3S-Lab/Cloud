import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CloudApi } from '../../lib/api';
import type { SearchResult } from '../../types/api';
import { ResourceSearch } from './resource-search';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const RESOURCE_ID = '019c0000-0000-7000-8000-000000000002';

let root: Root | null = null;

beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('ResourceSearch', () => {
  it('queries only the server-backed tenant search and selects a returned result', async () => {
    const host = testHost();
    const api = new CloudApi('token');
    const result = searchResult();
    const searchResources = vi.spyOn(api, 'searchResources').mockResolvedValue([result]);
    const onSelect = vi.fn();
    root = createRoot(host);
    await act(async () => {
      root?.render(<ResourceSearch api={api} organizationId={ORGANIZATION_ID} onSelect={onSelect} />);
    });

    const input = host.querySelector<HTMLInputElement>('input[type="search"]');
    if (!input) throw new Error('search input is missing');
    await changeInput(input, '  worker  ');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });

    expect(searchResources).toHaveBeenCalledOnce();
    expect(searchResources).toHaveBeenCalledWith(ORGANIZATION_ID, 'worker', 20, expect.any(AbortSignal));
    expect(host.textContent).toContain('Cloud worker');
    expect(host.textContent).toContain('Workload · desired running');

    const option = host.querySelector<HTMLButtonElement>('[role="option"]');
    await act(async () => option?.click());
    expect(onSelect).toHaveBeenCalledWith(result);
  });

  it('supports keyboard selection without issuing a broad resource read', async () => {
    const host = testHost();
    const api = new CloudApi('token');
    const result = searchResult();
    const searchResources = vi.spyOn(api, 'searchResources').mockResolvedValue([result]);
    const listWorkloads = vi.spyOn(api, 'listWorkloads');
    const onSelect = vi.fn();
    root = createRoot(host);
    await act(async () => {
      root?.render(<ResourceSearch api={api} organizationId={ORGANIZATION_ID} onSelect={onSelect} />);
    });

    const input = host.querySelector<HTMLInputElement>('input[type="search"]');
    if (!input) throw new Error('search input is missing');
    await changeInput(input, 'worker');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });

    expect(searchResources).toHaveBeenCalledOnce();
    expect(listWorkloads).not.toHaveBeenCalled();
    expect(onSelect).toHaveBeenCalledWith(result);
  });

  it('does not select a stale result while a changed query is settling', async () => {
    const host = testHost();
    const api = new CloudApi('token');
    const result = searchResult();
    vi.spyOn(api, 'searchResources').mockResolvedValue([result]);
    const onSelect = vi.fn();
    root = createRoot(host);
    await act(async () => {
      root?.render(<ResourceSearch api={api} organizationId={ORGANIZATION_ID} onSelect={onSelect} />);
    });

    const input = host.querySelector<HTMLInputElement>('input[type="search"]');
    if (!input) throw new Error('search input is missing');
    await changeInput(input, 'worker');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(host.querySelector('[role="option"]')).not.toBeNull();

    await changeInput(input, 'api');
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });

    expect(host.querySelector('[role="option"]')).toBeNull();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('rejects an oversized query before transport and disables search without organization context', async () => {
    const host = testHost();
    const api = new CloudApi('token');
    const searchResources = vi.spyOn(api, 'searchResources').mockResolvedValue([]);
    root = createRoot(host);
    await act(async () => {
      root?.render(<ResourceSearch api={api} organizationId={ORGANIZATION_ID} onSelect={vi.fn()} />);
    });

    const input = host.querySelector<HTMLInputElement>('input[type="search"]');
    if (!input) throw new Error('search input is missing');
    await changeInput(input, '界'.repeat(129));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });

    expect(searchResources).not.toHaveBeenCalled();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      'search query must contain 1 to 128 safe characters'
    );

    await act(async () => {
      root?.render(<ResourceSearch api={api} organizationId={null} onSelect={vi.fn()} />);
    });
    expect(host.querySelector<HTMLInputElement>('input[type="search"]')?.disabled).toBe(true);
  });
});

function testHost(): HTMLElement {
  const host = document.getElementById('root');
  if (!host) throw new Error('test root is missing');
  return host;
}

async function changeInput(input: HTMLInputElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  await act(async () => {
    setter?.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function searchResult(): SearchResult {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: '019c0000-0000-7000-8000-000000000003',
    environmentId: '019c0000-0000-7000-8000-000000000004',
    workloadId: RESOURCE_ID,
    kind: 'workload',
    id: RESOURCE_ID,
    title: 'Cloud worker',
    description: 'Workload · desired running',
    state: 'running',
    href: `#/organizations/${ORGANIZATION_ID}/workloads/${RESOURCE_ID}`,
    updatedAt: '2026-07-27T01:00:00.000Z',
  };
}
