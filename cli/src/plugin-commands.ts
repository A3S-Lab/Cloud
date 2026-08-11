import type {
  A3sUseJsonObject,
  CloudApi,
  PluginCatalogInspectRequest,
  PluginCatalogSearchRequest,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { usageError } from './errors';
import { isJsonObject, readBoundedJsonFile } from './json-file';
import { pluginCatalogResult, pluginRegistriesResult, pluginRegistryResult } from './plugin-results';
import type { CommandResult } from './results';

const MAX_PLUGIN_CATALOG_REQUEST_BYTES = 64 * 1024;

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
