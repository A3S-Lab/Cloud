import { CloudApiError, MAX_SECRET_VALUE_BYTES, type CloudApi } from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
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
import { readBoundedUtf8Stdin, type ReadStdin } from './standard-input';

const SECRET_VALUE_COMMANDS = new Set(['secrets create', 'secrets add-version']);

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
            positionalResourceName(positionals, 2),
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

async function readSecretValue(readStdin?: ReadStdin): Promise<string> {
  return readBoundedUtf8Stdin(readStdin, 1, MAX_SECRET_VALUE_BYTES, {
    read: 'unable to read Secret value from standard input',
    size: 'Secret value must contain between 1 byte and 1 MiB',
    utf8: 'Secret value must be valid UTF-8',
  });
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
