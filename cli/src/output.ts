import type { CliError } from './errors';

export interface TableColumn<Row> {
  header: string;
  value: (row: Row) => unknown;
}

const MAX_CELL_LENGTH = 96;
const MAX_DETAIL_DEPTH = 4;
const MAX_DETAIL_ENTRIES = 50;

export function renderJson(value: unknown): string {
  return `${JSON.stringify(value, null, 2)}\n`;
}

export function renderTable<Row>(rows: readonly Row[], columns: readonly TableColumn<Row>[]): string {
  const headers = columns.map((column) => sanitizeCell(column.header));
  const cells = rows.map((row) => columns.map((column) => sanitizeCell(column.value(row))));
  const widths = headers.map((header, columnIndex) =>
    Math.max(header.length, ...cells.map((row) => row[columnIndex]?.length ?? 0))
  );
  const lines = [
    formatRow(headers, widths),
    formatRow(
      widths.map((width) => '-'.repeat(width)),
      widths
    ),
  ];
  lines.push(...cells.map((row) => formatRow(row, widths)));
  return `${lines.join('\n')}\n`;
}

export function renderError(error: CliError, json: boolean): string {
  if (json) {
    return renderJson({
      error: {
        statusCode: error.statusCode,
        message: sanitizeCell(error.message),
        ...(error.status === undefined ? {} : { status: error.status }),
        ...(error.requestId === undefined ? {} : { requestId: sanitizeCell(error.requestId) }),
        ...(Object.keys(error.details).length === 0 ? {} : { details: sanitizeDetails(error.details) }),
      },
    });
  }
  const request = error.requestId ? ` (request ${sanitizeCell(error.requestId)})` : '';
  return `${error.statusCode}: ${sanitizeCell(error.message)}${request}\n`;
}

export function sanitizeCell(value: unknown): string {
  const source = value === null || value === undefined ? '' : String(value);
  let sanitized = '';
  for (const character of source) {
    const code = character.codePointAt(0) ?? 0;
    sanitized += code < 0x20 || (code >= 0x7f && code <= 0x9f) ? ' ' : character;
  }
  sanitized = sanitized.replace(/\s+/g, ' ').trim();
  const characters = Array.from(sanitized);
  if (characters.length > MAX_CELL_LENGTH) {
    return `${characters.slice(0, MAX_CELL_LENGTH - 1).join('')}…`;
  }
  return sanitized;
}

function formatRow(cells: readonly string[], widths: readonly number[]): string {
  return cells
    .map((cell, index) => cell.padEnd(widths[index] ?? cell.length))
    .join('  ')
    .trimEnd();
}

function sanitizeDetails(value: unknown, depth = 0): unknown {
  if (depth >= MAX_DETAIL_DEPTH) {
    return '[truncated]';
  }
  if (typeof value === 'string') {
    return sanitizeCell(value);
  }
  if (typeof value === 'number' || typeof value === 'boolean' || value === null) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.slice(0, MAX_DETAIL_ENTRIES).map((item) => sanitizeDetails(item, depth + 1));
  }
  if (typeof value === 'object' && value !== null) {
    return Object.fromEntries(
      Object.entries(value)
        .slice(0, MAX_DETAIL_ENTRIES)
        .map(([key, item]) => [
          sanitizeCell(key),
          isSensitiveKey(key) ? '[redacted]' : sanitizeDetails(item, depth + 1),
        ])
    );
  }
  return sanitizeCell(value);
}

function isSensitiveKey(key: string): boolean {
  const normalized = key
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/-/g, '_')
    .toLowerCase();
  return /(^|_)(authorization|password|private_key|secret|token)(_|$)/.test(normalized);
}
