import { CloudApiError, type CloudApi } from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireMutationCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { usageError } from './errors';
import { apiTokenMutationResult, apiTokenResult, apiTokensResult } from './identity-results';
import type { CommandResult } from './results';
import { readBoundedUtf8Stdin, type ReadStdin } from './standard-input';
import { parseRfc3339Timestamp } from './timestamp';

const API_TOKEN_CREATE_COMMAND = 'api-tokens create';

export interface IdentityCommandDependencies {
  readStdin?: ReadStdin;
}

export function rejectMisplacedIdentityOptions(command: string, arguments_: ParsedArguments): void {
  if (arguments_.tokenStdin && command !== API_TOKEN_CREATE_COMMAND) {
    throw usageError('--token-stdin is valid only for API token creation');
  }
  if (arguments_.scopes !== undefined && command !== API_TOKEN_CREATE_COMMAND) {
    throw usageError('--scopes and --expires-at are valid only for API token creation');
  }
  if (
    arguments_.expiresAt !== undefined &&
    command !== API_TOKEN_CREATE_COMMAND &&
    command !== 'nodes bootstrap'
  ) {
    throw usageError('--expires-at is valid only for API token creation or nodes bootstrap');
  }
}

export async function executeIdentityCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: IdentityCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'api-tokens list':
      requireListCommand(arguments_);
      return apiTokensResult(await cloudApi().listApiTokens(requireOrganization(context)));
    case 'api-tokens get':
      requireReadCommand(arguments_, 'api-tokens get <api-token-id>');
      return apiTokenResult(
        await cloudApi().getApiToken(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'API token ID')
        )
      );
    case API_TOKEN_CREATE_COMMAND: {
      const input = requireApiTokenCreateCommand(arguments_);
      const token = await readApiTokenCredential(dependencies.readStdin);
      return apiTokenMutationResult(
        await safeApiTokenMutation(() =>
          cloudApi().createApiToken(
            requireOrganization(context),
            {
              name: input.name,
              token,
              scopes: input.scopes,
              expiresAt: input.expiresAt,
            },
            input.idempotencyKey
          )
        )
      );
    }
    case 'api-tokens revoke': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'api-tokens revoke <api-token-id>');
      return apiTokenMutationResult(
        await safeApiTokenMutation(() =>
          cloudApi().revokeApiToken(
            requireOrganization(context),
            positionalUuid(positionals, 2, 'API token ID'),
            idempotencyKey
          )
        )
      );
    }
    default:
      return undefined;
  }
}

function requireApiTokenCreateCommand(arguments_: ParsedArguments): {
  name: string;
  scopes: string[];
  expiresAt: string | null;
  idempotencyKey: string;
} {
  requireArity(arguments_.positionals, 3, 'api-tokens create <name>');
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  if (!arguments_.tokenStdin) {
    throw usageError('--token-stdin is required for API token creation');
  }
  return {
    name: positionalResourceName(arguments_.positionals, 2),
    scopes: parseScopes(arguments_.scopes),
    expiresAt: parseExpiry(arguments_.expiresAt),
    idempotencyKey,
  };
}

function parseScopes(value: string | undefined): string[] {
  if (value === undefined) {
    throw usageError('--scopes is required for API token creation');
  }
  const scopes = value.split(',');
  const unique = new Set<string>();
  for (const scope of scopes) {
    if (scope.length > 63 || !/^[a-z-]+:[a-z-]+$/.test(scope)) {
      throw usageError('API token scope must use bounded lowercase domain:action syntax');
    }
    if (unique.has(scope)) {
      throw usageError('API token scopes must be unique');
    }
    unique.add(scope);
  }
  return scopes;
}

function parseExpiry(value: string | undefined): string | null {
  if (value === undefined) {
    return null;
  }
  return parseRfc3339Timestamp(value, 'API token');
}

async function readApiTokenCredential(readStdin?: ReadStdin): Promise<string> {
  const token = await readBoundedUtf8Stdin(readStdin, 68, 68, {
    read: 'unable to read API token credential from standard input',
    size: 'API token credential must contain exactly 68 bytes',
    utf8: 'API token credential must be valid UTF-8',
  });
  if (!/^a3s_[0-9a-f]{64}$/.test(token)) {
    throw usageError('API token must use the a3s_ prefix followed by 64 lowercase hex digits');
  }
  return token;
}

async function safeApiTokenMutation<Result>(operation: () => Promise<Result>): Promise<Result> {
  try {
    return await operation();
  } catch (error) {
    if (error instanceof CloudApiError) {
      throw new CloudApiError(error.status, 'API token mutation failed', error.statusCode, error.requestId);
    }
    throw error;
  }
}
