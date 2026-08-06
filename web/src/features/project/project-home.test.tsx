import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { LanguageProvider } from '../../lib/i18n';
import { ProjectHome } from './project-home';

let root: Root | null = null;

beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  localStorage.clear();
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  localStorage.clear();
});

describe('ProjectHome', () => {
  it('shows the Chinese-first product system, A3S Web, and architecture', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <LanguageProvider>
          <ProjectHome />
        </LanguageProvider>
      );
    });

    expect(host.querySelector('#home-title')?.textContent).toContain('企业级 AI 操作系统');
    expect(host.querySelectorAll('.product-pillar')).toHaveLength(3);
    expect(host.textContent).toContain('Workflow 自主工作流编排');
    expect(host.textContent).toContain('Agent Factory 异构智能体工厂');
    expect(host.textContent).toContain('A3S Gateway 统一网关');
    expect(host.textContent).toContain('Cloud API + A3S Gateway');
    expect(host.textContent).toContain('Sentry / AnySentry 安全证据');
    expect(host.querySelectorAll('.web-client-capability-grid article')).toHaveLength(6);
    expect(host.textContent).toContain('一个客户端，贯通三大产品的每一次工作');
    expect(host.querySelectorAll('.architecture-business-group li')).toHaveLength(19);
    expect(host.querySelector('#documentation')).toBeNull();
    expect(host.querySelector('#access')).toBeNull();
    expect(host.querySelector('.architecture-harness-card code')?.textContent).toBe(
      '/usr/bin/a3s code harness --manifest /app/.a3s/asset.acl'
    );
  });

  it('switches the complete public surface to English', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <LanguageProvider>
          <ProjectHome />
        </LanguageProvider>
      );
    });

    const englishButton = [...host.querySelectorAll<HTMLButtonElement>('.language-switcher button')].find(
      (button) => button.textContent === 'EN'
    );
    await act(async () => englishButton?.click());

    expect(host.querySelector('#home-title')?.textContent).toContain('The enterprise AI operating system');
    expect(host.textContent).toContain('Three products turn AI into an operable system');
    expect(host.textContent).toContain('One client for every action across all three products');
    expect(host.textContent).not.toContain('Documentation and versions');
  });
});
