import { type CloudApi, MAX_MCP_ROUTE_POLICY_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
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
import { requireEnvironment, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import {
  type CommandResult,
  domainClaimMutationResult,
  domainClaimResult,
  domainClaimsResult,
  gatewayScopeMutationResult,
  gatewayScopesResult,
  mcpCredentialDeliveryResult,
  mcpCredentialMutationResult,
  mcpCredentialResult,
  mcpCredentialsResult,
  mcpRoutePoliciesResult,
  mcpRoutePolicyMutationResult,
  mcpRoutePolicyResult,
  routePublicationResult,
} from './results';
import { parseRfc3339Timestamp } from './timestamp';

const MAX_GATEWAY_SCOPE_MEMBERS = 100;
const MAX_U32 = 4_294_967_295;

interface EdgeCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeEdgeCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: EdgeCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'domain-claims list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return domainClaimsResult(
        await cloudApi().listDomainClaims(scope.organizationId, scope.projectId, scope.environmentId)
      );
    }
    case 'domain-claims get':
      requireReadCommand(arguments_, 'domain-claims get <domain-claim-id>');
      return domainClaimResult(
        await cloudApi().getDomainClaim(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'domain claim ID')
        )
      );
    case 'domain-claims create': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'domain-claims create <domain-pattern>');
      const scope = requireEnvironmentScope(context);
      return domainClaimMutationResult(
        await cloudApi().createDomainClaim(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          canonicalDomainPattern(positionals[2]),
          idempotencyKey
        )
      );
    }
    case 'domain-claims verify': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'domain-claims verify <domain-claim-id> <proof>'
      );
      return domainClaimMutationResult(
        await cloudApi().verifyDomainClaim(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'domain claim ID'),
          boundedSingleLine(positionals[3], 512, 'domain ownership proof'),
          idempotencyKey
        )
      );
    }
    case 'domain-claims revoke': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'domain-claims revoke <domain-claim-id> <reason>'
      );
      return domainClaimMutationResult(
        await cloudApi().revokeDomainClaim(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'domain claim ID'),
          boundedSingleLine(positionals[3], 4_096, 'domain claim revocation reason'),
          idempotencyKey
        )
      );
    }
    case 'gateway-scopes list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return gatewayScopesResult(
        await cloudApi().listGatewayScopes(scope.organizationId, scope.projectId, scope.environmentId)
      );
    }
    case 'gateway-scopes create': {
      const mutation = requireGatewayScopeMutation(arguments_);
      const scope = requireEnvironmentScope(context);
      return gatewayScopeMutationResult(
        await cloudApi().createGatewayScope(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          {
            nodeIds: mutation.nodeIds,
            minReady: mutation.minReady,
            maxUnavailable: mutation.maxUnavailable,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'mcp-credentials list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return mcpCredentialsResult(
        await cloudApi().listMcpCredentials(scope.organizationId, scope.projectId, scope.environmentId)
      );
    }
    case 'mcp-credentials get':
      requireReadCommand(arguments_, 'mcp-credentials get <credential-id>');
      return mcpCredentialResult(
        await cloudApi().getMcpCredential(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'MCP credential ID')
        )
      );
    case 'mcp-credentials create': {
      const idempotencyKey = requireMutationCommand(arguments_, 2, 'mcp-credentials create');
      const scope = requireEnvironmentScope(context);
      return mcpCredentialDeliveryResult(
        await cloudApi().createMcpCredential(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          { expiresAt: parseRfc3339Timestamp(arguments_.expiresAt, 'MCP credential') },
          idempotencyKey
        )
      );
    }
    case 'mcp-credentials rotate': {
      const mutation = requireMcpCredentialVersionedMutation(
        arguments_,
        'mcp-credentials rotate <credential-id>'
      );
      return mcpCredentialDeliveryResult(
        await cloudApi().rotateMcpCredential(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'MCP credential ID'),
          {
            expiresAt: parseRfc3339Timestamp(arguments_.expiresAt, 'MCP credential'),
            expectedAggregateVersion: mutation.expectedVersion,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'mcp-credentials revoke': {
      const mutation = requireMcpCredentialVersionedMutation(
        arguments_,
        'mcp-credentials revoke <credential-id>'
      );
      return mcpCredentialMutationResult(
        await cloudApi().revokeMcpCredential(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'MCP credential ID'),
          { expectedAggregateVersion: mutation.expectedVersion },
          mutation.idempotencyKey
        )
      );
    }
    case 'mcp-routes list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return mcpRoutePoliciesResult(
        await cloudApi().listMcpRoutePolicies(scope.organizationId, scope.projectId, scope.environmentId)
      );
    }
    case 'mcp-routes get':
      requireReadCommand(arguments_, 'mcp-routes get <route-id>');
      return mcpRoutePolicyResult(
        await cloudApi().getMcpRoutePolicy(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'MCP route policy ID')
        )
      );
    case 'mcp-routes create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'mcp-routes create');
      const scope = requireEnvironmentScope(context);
      const acl = await readMcpRoutePolicyAcl(mutation.file, dependencies.readFile);
      return mcpRoutePolicyMutationResult(
        await cloudApi().createMcpRoutePolicyFromAcl(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          acl,
          mutation.idempotencyKey
        )
      );
    }
    case 'mcp-routes revise': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'mcp-routes revise <route-id>');
      const acl = await readMcpRoutePolicyAcl(mutation.file, dependencies.readFile);
      return mcpRoutePolicyMutationResult(
        await cloudApi().reviseMcpRoutePolicyFromAcl(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'MCP route policy ID'),
          acl,
          mutation.idempotencyKey
        )
      );
    }
    case 'routes publish': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        8,
        'routes publish <gateway-scope-id> <workload-revision-id> <domain-claim-id> <hostname> <path-prefix> <port-name>'
      );
      const scope = requireEnvironmentScope(context);
      return routePublicationResult(
        await cloudApi().publishRoute(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          {
            gatewayScopeId: positionalUuid(positionals, 2, 'Gateway scope ID'),
            workloadRevisionId: positionalUuid(positionals, 3, 'workload revision ID'),
            domainClaimId: positionalUuid(positionals, 4, 'domain claim ID'),
            hostname: canonicalHostname(positionals[5], 'route hostname'),
            pathPrefix: canonicalRoutePath(positionals[6]),
            portName: routePortName(positionals[7]),
          },
          idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function readMcpRoutePolicyAcl(
  path: string,
  readFile?: (path: string) => Promise<Uint8Array>
): Promise<string> {
  return readAclDocument(
    path,
    {
      label: 'MCP route policy ACL',
      maximumBytes: MAX_MCP_ROUTE_POLICY_ACL_BYTES,
    },
    readFile
  );
}

function requireMcpCredentialVersionedMutation(
  arguments_: ParsedArguments,
  usage: string
): { expectedVersion: number; idempotencyKey: string } {
  requireArity(arguments_.positionals, 3, usage);
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const rawVersion = arguments_.expectedVersion;
  if (rawVersion === undefined || !/^[0-9]+$/.test(rawVersion)) {
    throw usageError('--expected-version must be a positive safe integer for MCP credential mutation');
  }
  const expectedVersion = Number(rawVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError('--expected-version must be a positive safe integer for MCP credential mutation');
  }
  return { expectedVersion, idempotencyKey };
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

function requireGatewayScopeMutation(arguments_: ParsedArguments): {
  nodeIds: string[];
  minReady: number;
  maxUnavailable: number;
  idempotencyKey: string;
} {
  const { positionals } = arguments_;
  if (positionals.length < 3 || positionals.length > 2 + MAX_GATEWAY_SCOPE_MEMBERS) {
    throw usageError('gateway-scopes create requires between 1 and 100 node IDs');
  }
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const nodeIds = positionals
    .slice(2)
    .map((_value, index) => positionalUuid(positionals, index + 2, `Gateway member ${index + 1} node ID`));
  if (new Set(nodeIds).size !== nodeIds.length) {
    throw usageError('Gateway scope member node IDs must be unique');
  }
  const minReady = boundedInteger(arguments_.minReady, 1, '--min-ready');
  const maxUnavailable = boundedInteger(arguments_.maxUnavailable, 0, '--max-unavailable');
  if (minReady < 1 || minReady > nodeIds.length) {
    throw usageError('--min-ready must be positive and no greater than the member count');
  }
  if (maxUnavailable >= nodeIds.length) {
    throw usageError('--max-unavailable must be smaller than the member count');
  }
  return { nodeIds, minReady, maxUnavailable, idempotencyKey };
}

function boundedInteger(value: string | undefined, defaultValue: number, label: string): number {
  if (value === undefined) {
    return defaultValue;
  }
  if (!/^[0-9]+$/.test(value)) {
    throw usageError(`${label} must be an unsigned 32-bit integer`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed > MAX_U32) {
    throw usageError(`${label} must be an unsigned 32-bit integer`);
  }
  return parsed;
}

function canonicalDomainPattern(value: string | undefined): string {
  if (value === undefined) {
    throw usageError('domain pattern is required');
  }
  const normalized = value.trim().toLowerCase();
  const suffix = normalized.startsWith('*.') ? normalized.slice(2) : normalized;
  if (normalized.includes('*') && !normalized.startsWith('*.')) {
    throw usageError('domain pattern must be an exact DNS name or one leading wildcard');
  }
  canonicalHostname(suffix, 'domain pattern');
  if (normalized.startsWith('*.') && !suffix.includes('.')) {
    throw usageError('wildcard domain pattern must contain a registrable suffix');
  }
  return normalized;
}

function canonicalHostname(value: string | undefined, label: string): string {
  const normalized = value?.trim().toLowerCase();
  if (
    !normalized ||
    normalized.length > 253 ||
    normalized.endsWith('.') ||
    /^\d{1,3}(?:\.\d{1,3}){3}$/.test(normalized) ||
    normalized.split('.').some((part) => {
      return (
        part.length < 1 ||
        part.length > 63 ||
        part.startsWith('-') ||
        part.endsWith('-') ||
        !/^[a-z0-9-]+$/.test(part)
      );
    })
  ) {
    throw usageError(`${label} must be a canonical DNS name`);
  }
  return normalized;
}

function canonicalRoutePath(value: string | undefined): string {
  if (
    !value ||
    value.length > 2_048 ||
    !value.startsWith('/') ||
    /[\0\r\n`?#]/.test(value) ||
    value.includes('//') ||
    value.split('/').some((part) => part === '.' || part === '..') ||
    /%(?![0-9a-f]{2})/i.test(value)
  ) {
    throw usageError('route path must be a canonical absolute URL path prefix');
  }
  return value;
}

function routePortName(value: string | undefined): string {
  if (!value || value.length > 63 || !/^[a-z0-9._-]+$/.test(value)) {
    throw usageError('route port name must match a declared service port');
  }
  return value;
}

function boundedSingleLine(value: string | undefined, maxBytes: number, label: string): string {
  const normalized = value?.trim();
  if (
    !normalized ||
    normalized !== value ||
    new TextEncoder().encode(normalized).byteLength > maxBytes ||
    /[\0\r\n]/.test(normalized)
  ) {
    throw usageError(`${label} must be a bounded single-line value`);
  }
  return normalized;
}
