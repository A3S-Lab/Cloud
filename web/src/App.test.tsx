import { describe, expect, test } from 'bun:test';

describe('Studio shell', () => {
  test('ships a self-contained Dify-derived light workflow shell', async () => {
    const styles = await Bun.file(new URL('./styles.css', import.meta.url)).text();

    expect(styles).not.toContain('@import');
    expect(styles).not.toContain('fonts.googleapis.com');
    expect(styles).toContain('color-scheme: light');
    expect(styles).toContain('.product-rail');
    expect(styles).toContain('.node-library');
    expect(styles).toContain('.node-panel');
    expect(styles).toContain('.run-panel');
    expect(styles).not.toContain('.avatar');
    expect(styles).not.toContain('.empty-orbit');
  });
});
