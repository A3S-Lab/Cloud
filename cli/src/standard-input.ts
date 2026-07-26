import { usageError } from './errors';

export type ReadStdin = (limitBytes: number) => Promise<Uint8Array>;

export interface BoundedUtf8InputMessages {
  read: string;
  size: string;
  utf8: string;
}

export async function readBoundedUtf8Stdin(
  readStdin: ReadStdin | undefined,
  minBytes: number,
  maxBytes: number,
  messages: BoundedUtf8InputMessages
): Promise<string> {
  let bytes: Uint8Array;
  try {
    bytes = await (readStdin ?? readLocalStdin)(maxBytes + 1);
  } catch {
    throw usageError(messages.read);
  }
  if (!(bytes instanceof Uint8Array)) {
    throw usageError(messages.read);
  }
  if (bytes.byteLength < minBytes || bytes.byteLength > maxBytes) {
    bytes.fill(0);
    throw usageError(messages.size);
  }
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError(messages.utf8);
  } finally {
    bytes.fill(0);
  }
}

async function readLocalStdin(limitBytes: number): Promise<Uint8Array> {
  const reader = Bun.stdin.stream().getReader();
  const chunks: Uint8Array[] = [];
  let byteLength = 0;
  try {
    while (byteLength < limitBytes) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      const remaining = limitBytes - byteLength;
      const chunk = value.byteLength > remaining ? value.subarray(0, remaining) : value;
      chunks.push(chunk.slice());
      byteLength += chunk.byteLength;
      if (byteLength === limitBytes) {
        await reader.cancel();
        break;
      }
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(byteLength);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    chunk.fill(0);
    offset += chunk.byteLength;
  }
  return bytes;
}
