import { isValidIdempotencyKey } from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import { parseUuid } from './context';
import { usageError } from './errors';

export function requireListCommand(arguments_: ParsedArguments): void {
  requireArity(arguments_.positionals, 2, `${arguments_.positionals[0]} list`);
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
}

export function requireReadCommand(arguments_: ParsedArguments, usage: string, arity = 3): void {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
}

export function requireMutationCommand(arguments_: ParsedArguments, arity: number, usage: string): string {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  const key = requireIdempotencyKey(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  return key;
}

export function requireVersionedMutationCommand(
  arguments_: ParsedArguments,
  arity: number,
  usage: string,
  label: string
): { expectedVersion: number; idempotencyKey: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const expectedVersion = requireExpectedVersion(arguments_, label);
  return { expectedVersion, idempotencyKey: requireIdempotencyKey(arguments_) };
}

export function requireExpectedVersion(arguments_: ParsedArguments, label: string): number {
  const rawVersion = arguments_.expectedVersion;
  if (rawVersion === undefined || !/^[0-9]+$/u.test(rawVersion)) {
    throw usageError(`--expected-version must be a positive safe integer for ${label} mutation`);
  }
  const expectedVersion = Number(rawVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError(`--expected-version must be a positive safe integer for ${label} mutation`);
  }
  return expectedVersion;
}

export function requireIdempotencyKey(arguments_: ParsedArguments): string {
  const key = arguments_.idempotencyKey;
  if (key === undefined) {
    throw usageError('--idempotency-key is required for mutation commands');
  }
  if (!isValidIdempotencyKey(key)) {
    throw usageError('idempotency key is invalid');
  }
  return key;
}

export function requireArity(positionals: readonly string[], expected: number, usage: string): void {
  if (positionals.length !== expected) {
    throw usageError(`usage: a3s-cloud ${usage}`);
  }
}

export function positionalUuid(positionals: readonly string[], index: number, label: string): string {
  const value = positionals[index];
  if (!value) {
    throw usageError(`${label} is required`);
  }
  return parseUuid(value, label);
}

export function positionalResourceName(positionals: readonly string[], index: number): string {
  const name = positionals[index]?.trim();
  if (!name || [...name].length > 63 || /[\0\r\n]/.test(name)) {
    throw usageError('resource name must contain 1 to 63 visible characters');
  }
  return name;
}

export function rejectLogOptions(arguments_: ParsedArguments): void {
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor and stream options are valid only for log commands');
  }
  if (arguments_.limit !== undefined) {
    throw usageError('--limit is valid only for search and log commands');
  }
}

export function rejectIdempotencyOption(arguments_: ParsedArguments): void {
  if (arguments_.idempotencyKey !== undefined) {
    throw usageError('--idempotency-key is valid only for mutation commands');
  }
}

export function rejectFileOption(arguments_: ParsedArguments): void {
  if (arguments_.file !== undefined) {
    throw usageError('--file is valid only for file-backed mutation commands');
  }
}

export function rejectExpectedVersionOption(arguments_: ParsedArguments): void {
  if (arguments_.expectedVersion !== undefined) {
    throw usageError('--expected-version is valid only for versioned mutation commands');
  }
}

export function rejectGatewayRolloutOptions(arguments_: ParsedArguments): void {
  if (arguments_.minReady !== undefined || arguments_.maxUnavailable !== undefined) {
    throw usageError('--min-ready and --max-unavailable are valid only for gateway-scopes create');
  }
}
