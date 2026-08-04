export interface CloudSequenceQuery {
  cursor?: string;
  limit?: number;
}

export function encodeSequenceQuery(
  query: CloudSequenceQuery,
  label: string,
  maximumLimit: number
): URLSearchParams {
  const parameters = new URLSearchParams();
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 1_024 || hasUnsafeControl(query.cursor)) {
      throw new TypeError(`${label} cursor is invalid`);
    }
    parameters.set('cursor', query.cursor);
  }
  if (query.limit !== undefined) {
    if (!Number.isSafeInteger(query.limit) || query.limit < 1 || query.limit > maximumLimit) {
      throw new RangeError(`${label} limit must be between 1 and ${maximumLimit}`);
    }
    parameters.set('limit', String(query.limit));
  }
  return parameters;
}

export function encodeQueryParameters(parameters: URLSearchParams): string {
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
