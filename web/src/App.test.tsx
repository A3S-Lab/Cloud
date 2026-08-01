import { describe, expect, test } from 'bun:test';

describe('Studio shell', () => {
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
