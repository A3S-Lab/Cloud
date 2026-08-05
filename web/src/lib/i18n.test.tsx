import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { LanguageProvider, LanguageSwitcher, useI18n } from './i18n';

let root: Root | null = null;

beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  document.documentElement.lang = 'en';
  localStorage.clear();
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
    true;
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  localStorage.clear();
});

describe('LanguageProvider', () => {
  it('defaults to Simplified Chinese and switches the whole surface to English', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <LanguageProvider>
          <LanguageProbe />
        </LanguageProvider>
      );
    });

    expect(host.querySelector('output')?.textContent).toBe('概览');
    expect(document.documentElement.lang).toBe('zh-CN');
    expect(localStorage.getItem('a3s-cloud.language')).toBe('zh-CN');

    const englishButton = [...host.querySelectorAll('button')].find(
      (button) => button.textContent === 'EN'
    );
    await act(async () => englishButton?.click());

    expect(host.querySelector('output')?.textContent).toBe('Overview');
    expect(document.documentElement.lang).toBe('en');
    expect(localStorage.getItem('a3s-cloud.language')).toBe('en');
  });

  it('restores an explicitly selected language', async () => {
    localStorage.setItem('a3s-cloud.language', 'en');
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    root = createRoot(host);

    await act(async () => {
      root?.render(
        <LanguageProvider>
          <LanguageProbe />
        </LanguageProvider>
      );
    });

    expect(host.querySelector('output')?.textContent).toBe('Overview');
    expect(document.documentElement.lang).toBe('en');
    expect(host.querySelector("button[aria-pressed='true']")?.textContent).toBe('EN');
  });
});

function LanguageProbe() {
  const { t } = useI18n();
  return (
    <>
      <output>{t('Overview')}</output>
      <LanguageSwitcher />
    </>
  );
}
