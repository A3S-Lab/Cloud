import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ControlPlaneAccess } from './control-plane-access';

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

describe('ControlPlaneAccess', () => {
  it('composes the access form from Card, Field, Input, and Button contracts', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(<ControlPlaneAccess onAuthenticated={vi.fn()} />);
    });

    expect(host.querySelector('section.card.signin-card > header')).not.toBeNull();
    expect(host.querySelector('form > .field')).not.toBeNull();
    expect(host.querySelector('.field > input.input[type="password"]')).not.toBeNull();
    expect(host.querySelector('button.btn.primary-button[type="submit"]')).not.toBeNull();
  });
});
