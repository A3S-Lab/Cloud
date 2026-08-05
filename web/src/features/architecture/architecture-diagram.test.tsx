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
  it('renders the platform map as HTML with the sole Code Harness boundary', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => root?.render(<ArchitectureSection />));

    const diagram = host.querySelector<HTMLElement>('.architecture-diagram');
    expect(diagram).not.toBeNull();
    expect(diagram?.querySelector('img')).toBeNull();
    expect(diagram?.querySelectorAll('section')).toHaveLength(7);
    expect(diagram?.textContent).toContain('Operations + A3S Flow');
    expect(diagram?.textContent).toContain('Fleet node_commands');
    expect(diagram?.textContent).toContain('Outbound-only Node Agent');
    expect(diagram?.textContent).toContain('A3S Runtime Task / Service');
    expect(diagram?.textContent).toContain('A3S Box');
    expect(diagram?.textContent).toContain('Plugins (planned)');
    expect(diagram?.textContent).toContain('Data / Inference (planned)');
    expect(diagram?.querySelector('code')?.textContent).toBe(
      '/usr/bin/a3s code harness --manifest /app/.a3s/asset.acl'
    );
    expect(diagram?.textContent).toContain('Cloud only orchestrates and transports');
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
