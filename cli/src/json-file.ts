import { usageError } from './errors';

export type ReadJsonFile = (path: string) => Promise<Uint8Array>;

export interface BoundedJsonFileOptions {
  label: string;
  maximumBytes: number;
  readError?: string;
}

export async function readBoundedJsonFile(
  path: string,
  options: BoundedJsonFileOptions,
  readFile: ReadJsonFile = (value) => Bun.file(value).bytes()
): Promise<unknown> {
  let bytes: Uint8Array;
  try {
    bytes = await readFile(path);
  } catch {
    throw usageError(options.readError ?? `unable to read the ${options.label} file`);
  }
  if (bytes.byteLength < 1 || bytes.byteLength > options.maximumBytes) {
    throw usageError(`${options.label} must contain between 1 and ${options.maximumBytes} UTF-8 bytes`);
  }
  let decoded: string;
  try {
    decoded = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError(`${options.label} must be valid UTF-8`);
  }
  try {
    return JSON.parse(decoded);
  } catch {
    throw usageError(`${options.label} must be valid JSON transport`);
  }
}

export function isJsonObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
