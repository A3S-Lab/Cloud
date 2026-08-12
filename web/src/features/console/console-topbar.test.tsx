import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CloudApi } from '../../lib/api';
import { ConsoleTopbar } from './console-topbar';

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

describe('ConsoleTopbar', () => {
  it('composes workspace identity, resource search, status, and actions from reusable contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <ConsoleTopbar
          api={new CloudApi('token')}
          organizationId={null}
          streamState='live'
          drawerOpen={false}
          onSelectSearchResult={vi.fn()}
          onToggleDrawer={vi.fn()}
          onSignOut={vi.fn()}
        />
      );
    });

    expect(host.querySelector('header.workspace-header.topbar')).not.toBeNull();
    expect(host.querySelector('[data-workspace-leading].brand-lockup [data-brand-name]')).not.toBeNull();
    expect(host.querySelector('form.combobox.resource-search')).not.toBeNull();
    expect(host.querySelector('[data-workspace-actions] .button-group.language-switcher')).not.toBeNull();
    expect(host.querySelector('[data-workspace-actions] .status-badge[data-state="active"]')).not.toBeNull();
  });
});
