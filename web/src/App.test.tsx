import { describe, expect, test } from 'bun:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { App } from './App';

describe('Studio shell', () => {
  test('renders only the core workflow surfaces', () => {
    const markup = renderToStaticMarkup(createElement(App));

    expect(markup).toContain('aria-label="Node library"');
    expect(markup).toContain('Runtime node catalog');
    expect(markup).toContain('data-testid="workflow-canvas"');
    expect(markup).toContain('data-testid="run-workflow"');
    expect(markup).toContain('aria-label="Execution console"');
    expect(markup).toContain('aria-label="Node inspector"');
  });

  test('does not restore the non-functional console shell', () => {
    const markup = renderToStaticMarkup(createElement(App));

    expect(markup).not.toContain('<nav');
    expect(markup).not.toContain('Primary navigation');
    expect(markup).not.toContain('Runtime providers');
    expect(markup).not.toContain('aria-label="Account"');
    expect(markup).not.toContain('Search nodes');
  });

  test('does not depend on external UI fonts or removed shell styles', async () => {
    const styles = await Bun.file(new URL('./styles.css', import.meta.url)).text();

    expect(styles).not.toContain('@import');
    expect(styles).not.toContain('fonts.googleapis.com');
    expect(styles).not.toContain('.rail');
    expect(styles).not.toContain('.avatar');
    expect(styles).not.toContain('.search-box');
    expect(styles).not.toContain('.empty-orbit');
  });
});
