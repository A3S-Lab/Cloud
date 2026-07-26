import { CloudApiError, MAX_SECRET_VALUE_BYTES, type CloudApi } from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  requireListCommand,
  requireMutationCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import type { CommandResult } from './results';
import { secretDetailsResult, secretMutationResult, secretsResult } from './secret-results';

const SECRET_VALUE_COMMANDS = new Set(['secrets create', 'secrets add-version']);

export type ReadStdin = (limitBytes: number) => Promise<Uint8Array>;

export interface SecretCommandDependencies {
  readStdin?: ReadStdin;
}

export function rejectMisplacedSecretValueOption(command: string, arguments_: ParsedArguments): void {
  if (arguments_.valueStdin && !SECRET_VALUE_COMMANDS.has(command)) {
    throw usageError('--value-stdin is valid only for Secret value mutations');
  }
}

export async function executeSecretCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: SecretCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'secrets list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return secretsResult(
        await cloudApi().listSecrets(scope.organizationId, scope.projectId, scope.environmentId)
      );
    }
    case 'secrets get':
      requireReadCommand(arguments_, 'secrets get <secret-id>');
      return secretDetailsResult(
        await cloudApi().getSecret(requireOrganization(context), positionalUuid(positionals, 2, 'Secret ID'))
      );
    case 'secrets create': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'secrets create <name>');
      requireSecretStdin(arguments_);
      const scope = requireEnvironmentScope(context);
      const value = await readSecretValue(dependencies.readStdin);
      return secretMutationResult(
        await safeSecretMutation(() =>
          cloudApi().createSecret(
            scope.organizationId,
            scope.projectId,
            scope.environmentId,
            resourceName(positionals[2]),
            value,
            idempotencyKey
          )
        )
      );
    }
    case 'secrets add-version': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'secrets add-version <secret-id>');
      requireSecretStdin(arguments_);
      const organizationId = requireOrganization(context);
      const secretId = positionalUuid(positionals, 2, 'Secret ID');
      const value = await readSecretValue(dependencies.readStdin);
      return secretMutationResult(
        await safeSecretMutation(() =>
          cloudApi().addSecretVersion(organizationId, secretId, value, idempotencyKey)
        )
      );
    }
    case 'secrets revoke-version': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'secrets revoke-version <secret-id> <version>'
      );
      const organizationId = requireOrganization(context);
      const secretId = positionalUuid(positionals, 2, 'Secret ID');
      const version = positiveVersion(positionals[3]);
      return secretMutationResult(
        await cloudApi().revokeSecretVersion(organizationId, secretId, version, idempotencyKey)
      );
    }
    default:
      return undefined;
  }
}

function requireSecretStdin(arguments_: ParsedArguments): void {
  if (!arguments_.valueStdin) {
    throw usageError('--value-stdin is required for Secret value mutations');
  }
}

async function readSecretValue(readStdin: ReadStdin = readLocalStdin): Promise<string> {
  let bytes: Uint8Array;
  try {
    bytes = await readStdin(MAX_SECRET_VALUE_BYTES + 1);
  } catch {
    throw usageError('unable to read Secret value from standard input');
  }
  if (!(bytes instanceof Uint8Array)) {
    throw usageError('unable to read Secret value from standard input');
  }
  if (bytes.byteLength < 1 || bytes.byteLength > MAX_SECRET_VALUE_BYTES) {
    bytes.fill(0);
    throw usageError('Secret value must contain between 1 byte and 1 MiB');
  }
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError('Secret value must be valid UTF-8');
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

async function safeSecretMutation<Result>(operation: () => Promise<Result>): Promise<Result> {
  try {
    return await operation();
  } catch (error) {
    if (error instanceof CloudApiError) {
      throw new CloudApiError(error.status, 'Secret mutation failed', error.statusCode, error.requestId);
    }
    throw error;
  }
}

function resourceName(value: string | undefined): string {
  const name = value?.trim();
  if (!name || [...name].length > 63 || /[\0\r\n]/.test(name)) {
    throw usageError('resource name must contain 1 to 63 visible characters');
  }
  return name;
}

function positiveVersion(value: string | undefined): number {
  if (!value || !/^[0-9]+$/.test(value)) {
    throw usageError('Secret version must be a positive safe integer');
  }
  const version = Number(value);
  if (!Number.isSafeInteger(version) || version < 1) {
    throw usageError('Secret version must be a positive safe integer');
  }
  return version;
}

function requireEnvironmentScope(context: CloudContext): {
  organizationId: string;
  projectId: string;
  environmentId: string;
} {
  return {
    organizationId: requireOrganization(context),
    projectId: requireProject(context),
    environmentId: requireEnvironment(context),
  };
}
