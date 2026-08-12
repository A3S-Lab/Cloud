import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ArchitectureSection } from './architecture-diagram';
import { exportElementAsPng } from './architecture-export';

vi.mock('./architecture-export', () => ({
  exportElementAsPng: vi.fn(() => Promise.resolve()),
}));

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
  vi.clearAllMocks();
});

describe('ArchitectureSection', () => {
  it('renders the platform map with one Agent provider contract and the native Code provider', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => root?.render(<ArchitectureSection />));

    const diagram = host.querySelector<HTMLElement>('.architecture-diagram');
    expect(diagram).not.toBeNull();
    expect(diagram?.querySelector('img')).toBeNull();
    expect(diagram?.querySelectorAll(':scope > section')).toHaveLength(8);
    expect(diagram?.querySelectorAll('.architecture-product-grid li')).toHaveLength(3);
    expect(diagram?.textContent).toContain('Workflow autonomous orchestration');
    expect(diagram?.textContent).toContain('Agent Factory');
    expect(diagram?.textContent).toContain('A3S Gateway unified gateway');
    expect(diagram?.querySelectorAll('.architecture-business-group li')).toHaveLength(19);
    expect(host.querySelector('article.card.architecture-surface')).not.toBeNull();
    expect(host.querySelector('header.toolbar.architecture-toolbar[data-wrap="true"]')).not.toBeNull();
    expect(host.querySelectorAll('.architecture-legend .status-badge')).toHaveLength(4);
    expect(host.querySelectorAll('ol.stepper.architecture-runtime-path > li')).toHaveLength(5);
    expect(host.querySelector<HTMLOListElement>('ol.stepper.architecture-runtime-path')?.tabIndex).toBe(0);
    expect(host.querySelectorAll('.architecture-runtime-path [data-step-marker]')).toHaveLength(5);
    expect(diagram?.textContent).toContain('Operations + A3S Flow');
    expect(diagram?.textContent).toContain('Fleet node_commands');
    expect(diagram?.textContent).toContain('Outbound-only Node Agent');
    expect(diagram?.textContent).toContain('A3S Runtime Task / Service');
    expect(diagram?.textContent).toContain('A3S Box');
    expect(diagram?.textContent).toContain('Immutable objects + fenced mutable volumes');
    expect(diagram?.textContent).toContain('Foundation');
    expect(diagram?.textContent).toContain('Durable Agent execution');
    expect(diagram?.textContent).toContain('A3S Use plugin assignments');
    expect(diagram?.textContent).toContain('Inference profile');
    expect(diagram?.textContent).toContain('Ontology-driven Workflow');
    expect(diagram?.textContent).toContain('Governed self-evolution');
    expect(diagram?.textContent).toContain('Native Agent execution provider');
    expect(diagram?.querySelector('.architecture-harness-card code')?.textContent).toBe(
      '/usr/bin/a3s code harness --manifest /app/.a3s/asset.acl'
    );
    expect(diagram?.textContent).toContain('One Cloud lifecycle and provider conformance contract');
  });

  it('exports the same live diagram as a PNG', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);
    await act(async () => root?.render(<ArchitectureSection />));

    const button = [...host.querySelectorAll('button')].find((candidate) =>
      candidate.textContent?.includes('Export PNG')
    );
    await act(async () => {
      button?.click();
      await Promise.resolve();
    });

    const diagram = host.querySelector<HTMLElement>('.architecture-diagram');
    expect(exportElementAsPng).toHaveBeenCalledWith(diagram, 'a3s-cloud-module-architecture.png');
    expect(host.querySelector('[role="status"]')?.textContent).toBe('Architecture PNG exported.');
  });
});
