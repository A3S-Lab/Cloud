import { CloudApiError } from './error';
import type { ApiEnvelope, ApiErrorEnvelope } from './types';

const MAX_JSON_RESPONSE_CHARACTERS = 16 * 1_024 * 1_024;
const MAX_ERROR_DETAIL_DEPTH = 4;
const MAX_ERROR_DETAIL_ENTRIES = 50;
const MAX_ERROR_DETAIL_STRING_LENGTH = 1_024;

export async function readResponse<T>(response: Response): Promise<T> {
  let payload: unknown;
  try {
    const declaredLength = Number(response.headers.get('content-length'));
    if (Number.isFinite(declaredLength) && declaredLength > MAX_JSON_RESPONSE_CHARACTERS) {
      throw invalidResponse(response.status);
    }
    const body = await response.text();
    if (body.length > MAX_JSON_RESPONSE_CHARACTERS) {
      throw invalidResponse(response.status);
    }
    payload = JSON.parse(body) as unknown;
  } catch {
    throw invalidResponse(response.status);
  }

  if (response.ok) {
    if (!isApiEnvelope(payload) || payload.code !== response.status) {
      throw invalidResponse(response.status);
    }
    return payload.data as T;
  }

  if (!isApiErrorEnvelope(payload) || payload.code !== response.status) {
    throw invalidResponse(response.status);
  }
  throw new CloudApiError(
    response.status,
    payload.message,
    payload.statusCode,
    payload.requestId,
    boundErrorDetails(payload.details)
  );
}

function isApiEnvelope(value: unknown): value is ApiEnvelope<unknown> {
  return (
    isRecord(value) &&
    Number.isSafeInteger(value.code) &&
    isBoundedText(value.message, 1_024) &&
    Object.hasOwn(value, 'data') &&
    isBoundedText(value.requestId, 256) &&
    isBoundedText(value.timestamp, 64)
  );
}

function isApiErrorEnvelope(value: unknown): value is ApiErrorEnvelope {
  return (
    isRecord(value) &&
    Number.isSafeInteger(value.code) &&
    isStatusCode(value.statusCode) &&
    isBoundedText(value.message, 1_024) &&
    isRecord(value.details) &&
    isBoundedText(value.requestId, 256) &&
    isBoundedText(value.timestamp, 64)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isBoundedText(value: unknown, maxLength: number): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= maxLength &&
    !containsControlCharacter(value)
  );
}

function isStatusCode(value: unknown): value is string {
  return typeof value === 'string' && /^[A-Z][A-Z0-9_]{0,63}$/.test(value);
}

function invalidResponse(status: number): CloudApiError {
  return new CloudApiError(status, 'Cloud API returned an invalid response', 'INVALID_RESPONSE');
}

function boundErrorDetails(details: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(details)
      .slice(0, MAX_ERROR_DETAIL_ENTRIES)
      .map(([key, value]) => [sanitizeMetadataText(key, 128), boundErrorDetailValue(value, 1)])
  );
}

function boundErrorDetailValue(value: unknown, depth: number): unknown {
  if (depth >= MAX_ERROR_DETAIL_DEPTH) {
    return '[truncated]';
  }
  if (typeof value === 'string') {
    return sanitizeMetadataText(value, MAX_ERROR_DETAIL_STRING_LENGTH);
  }
  if (typeof value === 'number' || typeof value === 'boolean' || value === null) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.slice(0, MAX_ERROR_DETAIL_ENTRIES).map((item) => boundErrorDetailValue(item, depth + 1));
  }
  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value)
        .slice(0, MAX_ERROR_DETAIL_ENTRIES)
        .map(([key, item]) => [sanitizeMetadataText(key, 128), boundErrorDetailValue(item, depth + 1)])
    );
  }
  return '[unsupported]';
}

function sanitizeMetadataText(value: string, maxLength: number): string {
  let sanitized = '';
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    sanitized += code < 0x20 || (code >= 0x7f && code <= 0x9f) ? ' ' : character;
  }
  const characters = Array.from(sanitized);
  return characters.length <= maxLength ? sanitized : `${characters.slice(0, maxLength - 1).join('')}…`;
}

function containsControlCharacter(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x20 || (code >= 0x7f && code <= 0x9f)) {
      return true;
    }
  }
  return false;
}
