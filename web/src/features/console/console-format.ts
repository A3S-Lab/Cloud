import type { StreamState } from '../operations/use-operation-stream';

export function humanize(value: string): string {
  return value.replaceAll('_', ' ').replace(/^./, (character) => character.toUpperCase());
}

export function formatRelative(value: string): string {
  const elapsed = Math.max(0, Date.now() - new Date(value).getTime());
  if (elapsed < 60_000) return 'just now';
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m ago`;
  return new Intl.DateTimeFormat('en', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value));
}

export function formatTimestamp(value: string | null): string {
  if (!value) return 'Not recorded';
  return new Intl.DateTimeFormat('en', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(value));
}

export function shortId(value: string): string {
  return value.slice(0, 8);
}

export function compactDigest(value: string): string {
  const separator = value.indexOf(':');
  if (separator === -1) return shortId(value);
  return `${value.slice(0, separator + 1)}${value.slice(separator + 1, separator + 9)}`;
}

export function streamLabel(state: StreamState): string {
  if (state === 'live') return 'Live';
  if (state === 'retrying') return 'Reconnecting';
  if (state === 'connecting') return 'Connecting';
  return 'Idle';
}
