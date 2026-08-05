import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SearchResourceKind } from '../../types/api';
import { ConsoleNavigation, sectionForResourceKind } from './console-navigation';

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

describe('ConsoleNavigation', () => {
  it.each<[SearchResourceKind | null, string]>([
    [null, 'overview'],
    ['project', 'overview'],
    ['environment', 'overview'],
    ['node', 'overview'],
    ['secret', 'overview'],
    ['operation', 'overview'],
    ['workload', 'workloads'],
    ['deployment', 'workloads'],
    ['build_run', 'delivery'],
    ['source_revision', 'delivery'],
    ['route', 'edge'],
    ['domain_claim', 'edge'],
    ['gateway_scope', 'edge'],
  ])('maps %s resources to the %s section', (kind, expected) => {
    expect(sectionForResourceKind(kind)).toBe(expected);
  });

  it('exposes one active section and reports selection through buttons', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const onSelect = vi.fn();
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <ConsoleNavigation
          activeSection='workloads'
          counts={{ workloads: 3, agents: 5, delivery: 2, edge: 1, operations: 4 }}
          onSelect={onSelect}
        />
      );
    });

    const buttons = [...host.querySelectorAll('button')];
    expect(buttons.map((button) => button.querySelector('strong')?.textContent)).toEqual([
      'Overview',
      'Workloads',
      'Agents',
      'Delivery',
      'Edge',
      'Architecture',
    ]);
    expect(buttons.map((button) => button.querySelector('em')?.textContent)).toEqual([
      '4',
      '3',
      '5',
      '2',
      '1',
      undefined,
    ]);
    expect(buttons[1]?.getAttribute('aria-label')).toBe('Workloads, Runtime convergence, 3 workloads');
    expect(buttons.filter((button) => button.getAttribute('aria-current') === 'page')).toHaveLength(1);
    expect(buttons[1]?.getAttribute('aria-current')).toBe('page');
    expect(buttons[5]?.getAttribute('aria-label')).toBe('Architecture, Platform module map');
    expect(buttons.map((button) => button.tabIndex)).toEqual([-1, 0, -1, -1, -1, -1]);

    await act(async () => buttons[2]?.click());
    expect(onSelect).toHaveBeenCalledWith('agents');
  });

  it('supports automatic keyboard selection with wrapping and boundary keys', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const onSelect = vi.fn();
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <ConsoleNavigation
          activeSection='overview'
          counts={{ workloads: 3, agents: 5, delivery: 2, edge: 1, operations: 4 }}
          onSelect={onSelect}
        />
      );
    });

    const buttons = [...host.querySelectorAll('button')];
    buttons[0]?.focus();
    await act(async () =>
      buttons[0]?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }))
    );
    expect(onSelect).toHaveBeenLastCalledWith('architecture');
    expect(document.activeElement).toBe(buttons[5]);

    await act(async () =>
      buttons[5]?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }))
    );
    expect(onSelect).toHaveBeenLastCalledWith('overview');
    expect(document.activeElement).toBe(buttons[0]);

    await act(async () =>
      buttons[0]?.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }))
    );
    expect(onSelect).toHaveBeenLastCalledWith('architecture');
    expect(document.activeElement).toBe(buttons[5]);
  });
});
