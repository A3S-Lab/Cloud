import type { ParsedArguments } from './arguments';
import {
  rejectExpectedVersionOption,
  rejectGatewayRolloutOptions,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
} from './command-options';
import { usageError } from './errors';

export interface AclDocumentConstraint {
  label: string;
  maximumBytes: number;
}

export function requireAclMutationCommand(
  arguments_: ParsedArguments,
  arity: number,
  usage: string
): { idempotencyKey: string; file: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const file = arguments_.file;
  if (file === undefined) {
    throw usageError('--file is required for ACL desired-state mutations');
  }
  if (file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('ACL file path is invalid');
  }
  return { idempotencyKey, file };
}

export async function readAclDocument(
  path: string,
  constraint: AclDocumentConstraint,
  readFile: (path: string) => Promise<Uint8Array> = readLocalFile
): Promise<string> {
  let bytes: Uint8Array;
  try {
    bytes = await readFile(path);
  } catch {
    throw usageError('unable to read the A3S ACL file');
  }
  if (bytes.byteLength < 1 || bytes.byteLength > constraint.maximumBytes) {
    throw usageError(`${constraint.label} must contain between 1 and ${constraint.maximumBytes} UTF-8 bytes`);
  }
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError(`${constraint.label} must be valid UTF-8`);
  }
}

async function readLocalFile(path: string): Promise<Uint8Array> {
  return Bun.file(path).bytes();
}
