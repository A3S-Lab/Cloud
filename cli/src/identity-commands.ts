import { type CloudApi, CloudApiError, type MembershipRole } from '@a3s/cloud-client';
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
import {
  apiTokenMutationResult,
  apiTokenResult,
  apiTokensResult,
  membershipMutationResult,
  membershipResult,
  membershipsResult,
} from './identity-results';
import type { CommandResult } from './results';
import { type ReadStdin, readBoundedUtf8Stdin } from './standard-input';
import { parseRfc3339Timestamp } from './timestamp';

const API_TOKEN_CREATE_COMMAND = 'api-tokens create';
const MCP_EXPIRY_COMMANDS = new Set(['mcp-credentials create', 'mcp-credentials rotate']);

export interface IdentityCommandDependencies {
  readStdin?: ReadStdin;
}

export function rejectMisplacedIdentityOptions(command: string, arguments_: ParsedArguments): void {
  if (arguments_.tokenStdin && command !== API_TOKEN_CREATE_COMMAND) {
    throw usageError('--token-stdin is valid only for API token creation');
  }
  if (arguments_.scopes !== undefined && command !== API_TOKEN_CREATE_COMMAND) {
    throw usageError('--scopes is valid only for API token creation');
  }
  if (arguments_.apiTokenPrincipalId !== undefined && command !== API_TOKEN_CREATE_COMMAND) {
    throw usageError('--principal is valid only for API token creation');
  }
  if (
    arguments_.expiresAt !== undefined &&
    command !== API_TOKEN_CREATE_COMMAND &&
    command !== 'nodes bootstrap' &&
    !MCP_EXPIRY_COMMANDS.has(command)
  ) {
    throw usageError(
      '--expires-at is valid only for API token creation, node bootstrap, or MCP credential creation and rotation'
    );
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
              principalId: input.principalId,
              expiresAt: input.expiresAt,
            },
            input.idempotencyKey
          )
        )
      );
    }
    case 'memberships list':
      requireListCommand(arguments_);
      return membershipsResult(await cloudApi().listMemberships(requireOrganization(context)));
    case 'memberships get':
      requireReadCommand(arguments_, 'memberships get <membership-id>');
      return membershipResult(
        await cloudApi().getMembership(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'membership ID')
        )
      );
    case 'memberships create-service': {
      requireArity(positionals, 4, 'memberships create-service <name> <role>');
      rejectLogOptions(arguments_);
      rejectFileOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      const idempotencyKey = requireIdempotencyKey(arguments_);
      return membershipMutationResult(
        await safeMembershipMutation(() =>
          cloudApi().createServiceMembership(
            requireOrganization(context),
            {
              name: positionalResourceName(positionals, 2),
              role: membershipRole(positionals[3]),
            },
            idempotencyKey
          )
        )
      );
    }
    case 'memberships change-role': {
      const mutation = requireMembershipVersionMutation(
        arguments_,
        4,
        'memberships change-role <membership-id> <role>'
      );
      return membershipMutationResult(
        await safeMembershipMutation(() =>
          cloudApi().changeMembershipRole(
            requireOrganization(context),
            positionalUuid(positionals, 2, 'membership ID'),
            membershipRole(positionals[3]),
            mutation.expectedVersion,
            mutation.idempotencyKey
          )
        )
      );
    }
    case 'memberships revoke': {
      const mutation = requireMembershipVersionMutation(arguments_, 3, 'memberships revoke <membership-id>');
      return membershipMutationResult(
        await safeMembershipMutation(() =>
          cloudApi().revokeMembership(
            requireOrganization(context),
            positionalUuid(positionals, 2, 'membership ID'),
            mutation.expectedVersion,
            mutation.idempotencyKey
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
  principalId?: string;
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
    principalId:
      arguments_.apiTokenPrincipalId === undefined
        ? undefined
        : positionalUuid([arguments_.apiTokenPrincipalId], 0, 'principal ID'),
    idempotencyKey,
  };
}

function requireMembershipVersionMutation(
  arguments_: ParsedArguments,
  arity: number,
  usage: string
): { expectedVersion: number; idempotencyKey: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const expectedVersion = Number(arguments_.expectedVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError('--expected-version must be a positive safe integer for membership mutation');
  }
  return { expectedVersion, idempotencyKey: requireIdempotencyKey(arguments_) };
}

function membershipRole(value: string | undefined): MembershipRole {
  if (!value || !['owner', 'admin', 'member', 'restricted'].includes(value)) {
    throw usageError('membership role must be owner, admin, member, or restricted');
  }
  return value as MembershipRole;
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

async function safeMembershipMutation<Result>(operation: () => Promise<Result>): Promise<Result> {
  try {
    return await operation();
  } catch (error) {
    if (error instanceof CloudApiError) {
      throw new CloudApiError(error.status, 'membership mutation failed', error.statusCode, error.requestId);
    }
    throw error;
  }
}
