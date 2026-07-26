import { describe, expect, it } from 'bun:test';
import { CliError } from '../src/errors';
import { renderError, renderTable, sanitizeCell } from '../src/output';

describe('CLI output', () => {
  it('neutralizes terminal control characters and bounds cells', () => {
    expect(sanitizeCell('\u001b[31mnode\nname\u0000')).toBe('[31mnode name');
    expect(sanitizeCell('x'.repeat(200))).toHaveLength(96);
    expect(sanitizeCell('x'.repeat(200))).toEndWith('…');
  });

  it('renders deterministic tables including an empty result', () => {
    expect(
      renderTable(
        [],
        [
          { header: 'ID', value: (row: { id: string; name: string }) => row.id },
          { header: 'NAME', value: (row: { id: string; name: string }) => row.name },
        ]
      )
    ).toBe('ID  NAME\n--  ----\n');
  });

  it('redacts sensitive error-detail keys in JSON output', () => {
    const error = new CliError(6, 'BAD_GATEWAY', 'upstream failed', {
      status: 502,
      requestId: 'request-1',
      details: {
        retry: true,
        token: 'a3s_secret',
        nested: { authorizationHeader: 'Bearer secret' },
      },
    });

    const rendered = renderError(error, true);

    expect(rendered).toContain('"retry": true');
    expect(rendered).toContain('[redacted]');
    expect(rendered).not.toContain('a3s_secret');
    expect(rendered).not.toContain('Bearer secret');
  });
});
