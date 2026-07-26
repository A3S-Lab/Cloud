import type { WorkloadLogStreamFilter } from './types';

export interface CloudLogQuery {
  cursor?: string;
  limit?: number;
  stream?: WorkloadLogStreamFilter;
}

export function encodeLogQuery(query: CloudLogQuery): string {
  const parameters = new URLSearchParams();
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 1_024 || hasUnsafeControl(query.cursor)) {
      throw new TypeError('log cursor is invalid');
    }
    parameters.set('cursor', query.cursor);
  }
  if (query.limit !== undefined) {
    if (!Number.isSafeInteger(query.limit) || query.limit < 1 || query.limit > 256) {
      throw new RangeError('log limit must be between 1 and 256');
    }
    parameters.set('limit', String(query.limit));
  }
  if (query.stream !== undefined) {
    if (query.stream !== 'stdout' && query.stream !== 'stderr') {
      throw new TypeError('log stream must be stdout or stderr');
    }
    parameters.set('stream', query.stream);
  }
  const encoded = parameters.toString();
  return encoded ? `?${encoded}` : '';
}

function hasUnsafeControl(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (code <= 0x20 || (code >= 0x7f && code <= 0x9f)) {
      return true;
    }
  }
  return false;
}
