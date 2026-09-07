import type {
  A3sUseJsonObject,
  CloudApi,
  ConfirmPluginPlanProjectionInput,
  PluginCatalogInspectRequest,
  PluginCatalogSearchRequest,
  SetPluginAssignmentInput,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import { isJsonObject, readBoundedJsonFile } from './json-file';
import {
  pluginAssignmentMutationResult,
  pluginAssignmentResult,
  pluginAssignmentsResult,
  pluginCatalogResult,
  pluginPlanProjectionResult,
  pluginRegistriesResult,
  pluginRegistryResult,
} from './plugin-results';
import type { CommandResult } from './results';

const MAX_PLUGIN_CATALOG_REQUEST_BYTES = 64 * 1024;
const MAX_PLUGIN_ASSIGNMENT_REQUEST_BYTES = 64 * 1024;

interface PluginCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executePluginCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: PluginCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'plugin-registries list':
      requireListCommand(arguments_);
      return pluginRegistriesResult(await cloudApi().listPluginRegistries(requireOrganization(context)));
    case 'plugin-registries get':
      requireReadCommand(arguments_, 'plugin-registries get <registry-id>');
      return pluginRegistryResult(
        await cloudApi().getPluginRegistry(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Plugin Registry ID')
        )
      );
    case 'plugin-assignments list': {
      requireListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return pluginAssignmentsResult(
        await cloudApi().listPluginAssignments(
          scope.organizationId,
          scope.projectId,
          scope.environmentId
        )
      );
    }
    case 'plugin-assignments get': {
      requireReadCommand(arguments_, 'plugin-assignments get <assignment-id>');
      const scope = requireEnvironmentScope(context);
      return pluginAssignmentResult(
        await cloudApi().getPluginAssignment(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'Plugin Assignment ID')
        )
      );
    }
    case 'plugin-assignments set': {
      const mutation = requirePluginAssignmentMutation(arguments_);
      const scope = requireEnvironmentScope(context);
      const input = await readPluginAssignmentInput(mutation.file, dependencies.readFile);
      return pluginAssignmentMutationResult(
        await cloudApi().setPluginAssignment(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          input,
          mutation.idempotencyKey
        )
      );
    }
    case 'plugin-plan-projections get': {
      requireReadCommand(arguments_, 'plugin-plan-projections get <projection-id>');
      return pluginPlanProjectionResult(
        await cloudApi().getPluginPlanProjection(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Plugin Plan Projection ID')
        )
      );
    }
    case 'plugin-plan-projections confirm': {
      const mutation = requirePluginPlanConfirmationMutation(arguments_);
      const input = await readPluginPlanConfirmationInput(mutation.file, dependencies.readFile);
      return pluginPlanProjectionResult(
        await cloudApi().confirmPluginPlanProjection(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Plugin Plan Projection ID'),
          input,
          mutation.idempotencyKey
        )
      );
    }
    case 'plugin-catalog search':
    case 'plugin-catalog search-cached': {
      const request = (await readCatalogRequest(
        arguments_,
        `${command} <registry-id>`,
        dependencies.readFile
      )) as PluginCatalogSearchRequest;
      const organizationId = requireOrganization(context);
      const registryId = positionalUuid(positionals, 2, 'Plugin Registry ID');
      const result =
        command === 'plugin-catalog search'
          ? await cloudApi().searchPluginCatalog(organizationId, registryId, request)
          : await cloudApi().searchCachedPluginCatalog(organizationId, registryId, request);
      return pluginCatalogResult(result);
    }
    case 'plugin-catalog inspect':
    case 'plugin-catalog inspect-cached': {
      const request = (await readCatalogRequest(
        arguments_,
        `${command} <registry-id>`,
        dependencies.readFile
      )) as PluginCatalogInspectRequest;
      const organizationId = requireOrganization(context);
      const registryId = positionalUuid(positionals, 2, 'Plugin Registry ID');
      const result =
        command === 'plugin-catalog inspect'
          ? await cloudApi().inspectPluginCatalog(organizationId, registryId, request)
          : await cloudApi().inspectCachedPluginCatalog(organizationId, registryId, request);
      return pluginCatalogResult(result);
    }
    default:
      return undefined;
  }
}

async function readCatalogRequest(
  arguments_: ParsedArguments,
  usage: string,
  readFile?: (path: string) => Promise<Uint8Array>
): Promise<A3sUseJsonObject> {
  requireArity(arguments_.positionals, 3, usage);
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const path = arguments_.file;
  if (path === undefined || path.length > 4_096 || /[\0\r\n]/.test(path)) {
    throw usageError('--file with a valid A3S Use catalog request JSON path is required');
  }
  const value = await readBoundedJsonFile(
    path,
    {
      label: 'Plugin catalog request',
      maximumBytes: MAX_PLUGIN_CATALOG_REQUEST_BYTES,
    },
    readFile
  );
  if (!isJsonObject(value)) {
    throw usageError('Plugin catalog request must be a JSON object');
  }
  return value;
}

function requirePluginAssignmentMutation(arguments_: ParsedArguments): {
  idempotencyKey: string;
  file: string;
} {
  requireArity(arguments_.positionals, 2, 'plugin-assignments set');
  rejectLogOptions(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const file = arguments_.file;
  if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('--file with a valid plugin assignment JSON path is required');
  }
  return { idempotencyKey, file };
}

async function readPluginAssignmentInput(
  path: string,
  readFile?: (path: string) => Promise<Uint8Array>
): Promise<SetPluginAssignmentInput> {
  const value = await readBoundedJsonFile(
    path,
    {
      label: 'Plugin assignment input',
      maximumBytes: MAX_PLUGIN_ASSIGNMENT_REQUEST_BYTES,
      readError: 'unable to read the plugin assignment JSON file',
    },
    readFile
  );
  if (!isSetPluginAssignmentInput(value)) {
    throw usageError(
      'Plugin assignment input must contain registryId, targetHostId, workspaceScope, selection, policyDigest, and desiredState'
    );
  }
  return value;
}

function isSetPluginAssignmentInput(value: unknown): value is SetPluginAssignmentInput {
  if (!isJsonObject(value)) {
    return false;
  }
  return (
    typeof value.registryId === 'string' &&
    typeof value.targetHostId === 'string' &&
    isJsonObject(value.workspaceScope) &&
    isJsonObject(value.selection) &&
    typeof value.policyDigest === 'string' &&
    (value.desiredState === 'enabled' ||
      value.desiredState === 'installed-disabled' ||
      value.desiredState === 'absent')
  );
}

function requirePluginPlanConfirmationMutation(arguments_: ParsedArguments): {
  idempotencyKey: string;
  file: string;
} {
  requireArity(arguments_.positionals, 3, 'plugin-plan-projections confirm <projection-id>');
  rejectLogOptions(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const file = arguments_.file;
  if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('--file with a valid plugin plan confirmation JSON path is required');
  }
  return { idempotencyKey, file };
}

async function readPluginPlanConfirmationInput(
  path: string,
  readFile?: (path: string) => Promise<Uint8Array>
): Promise<ConfirmPluginPlanProjectionInput> {
  const value = await readBoundedJsonFile(
    path,
    {
      label: 'Plugin plan confirmation input',
      maximumBytes: MAX_PLUGIN_ASSIGNMENT_REQUEST_BYTES,
      readError: 'unable to read the plugin plan confirmation JSON file',
    },
    readFile
  );
  if (!isConfirmPluginPlanProjectionInput(value)) {
    throw usageError('Plugin plan confirmation input must contain a confirmation object');
  }
  return value;
}

function isConfirmPluginPlanProjectionInput(
  value: unknown
): value is ConfirmPluginPlanProjectionInput {
  return isJsonObject(value) && isJsonObject(value.confirmation);
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
