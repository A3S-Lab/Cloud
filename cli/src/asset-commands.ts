import { type AssetKind, type CloudApi, MAX_MCP_SERVICE_PROFILE_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireListCommand,
  requireMutationCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { usageError } from './errors';
import {
  assetMutationResult,
  assetReleaseMutationResult,
  assetReleaseResult,
  assetReleasesResult,
  assetResult,
  assetsResult,
  mcpServiceProfileMutationResult,
  mcpServiceProfileResult,
} from './asset-results';
import type { CommandResult } from './results';

interface AssetCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeAssetCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: AssetCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  switch (command) {
    case 'assets list':
      requireListCommand(arguments_);
      return assetsResult(await cloudApi().listAssets(organizationId()));
    case 'assets get':
      requireReadCommand(arguments_, 'assets get <asset-id>');
      return assetResult(
        await cloudApi().getAsset(organizationId(), positionalUuid(positionals, 2, 'Asset ID'))
      );
    case 'assets create': {
      const idempotencyKey = requireMutationCommand(arguments_, 4, 'assets create <name> <agent|mcp|skill>');
      return assetMutationResult(
        await cloudApi().createAsset(
          organizationId(),
          {
            name: positionalResourceName(positionals, 2),
            kind: assetKind(positionals[3]),
          },
          idempotencyKey
        )
      );
    }
    case 'assets archive': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'assets archive <asset-id>');
      return assetMutationResult(
        await cloudApi().archiveAsset(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          idempotencyKey
        )
      );
    }
    case 'asset-releases list':
      requireReadCommand(arguments_, 'asset-releases list <asset-id>');
      return assetReleasesResult(
        await cloudApi().listAssetReleases(organizationId(), positionalUuid(positionals, 2, 'Asset ID'))
      );
    case 'asset-releases get':
      requireAssetRead(arguments_, [4], 'asset-releases get <asset-id> <release-id>');
      return assetReleaseResult(
        await cloudApi().getAssetRelease(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          positionalUuid(positionals, 3, 'Asset release ID')
        )
      );
    case 'asset-releases mcp-profile':
      requireAssetRead(arguments_, [4], 'asset-releases mcp-profile <asset-id> <release-id>');
      return mcpServiceProfileResult(
        await cloudApi().getMcpServiceProfile(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          positionalUuid(positionals, 3, 'Asset release ID')
        )
      );
    case 'asset-releases select':
      requireAssetRead(arguments_, [3, 4], 'asset-releases select <asset-id> [version]');
      return assetReleaseResult(
        await cloudApi().selectAssetRelease(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          positionals[3] === undefined ? undefined : releaseVersion(positionals[3])
        )
      );
    case 'asset-releases create': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        5,
        'asset-releases create <asset-id> <version> <commit-sha>'
      );
      return assetReleaseMutationResult(
        await cloudApi().createAssetRelease(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          {
            version: releaseVersion(positionals[3]),
            commitSha: commitSha(positionals[4]),
          },
          idempotencyKey
        )
      );
    }
    case 'asset-releases yank': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'asset-releases yank <asset-id> <release-id>'
      );
      return assetReleaseMutationResult(
        await cloudApi().yankAssetRelease(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          positionalUuid(positionals, 3, 'Asset release ID'),
          idempotencyKey
        )
      );
    }
    case 'asset-releases bind-mcp-profile': {
      const mutation = requireAclMutationCommand(
        arguments_,
        4,
        'asset-releases bind-mcp-profile <asset-id> <release-id>'
      );
      const acl = await readAclDocument(
        mutation.file,
        {
          label: 'MCP Service profile ACL',
          maximumBytes: MAX_MCP_SERVICE_PROFILE_ACL_BYTES,
        },
        dependencies.readFile
      );
      return mcpServiceProfileMutationResult(
        await cloudApi().bindMcpServiceProfileFromAcl(
          organizationId(),
          positionalUuid(positionals, 2, 'Asset ID'),
          positionalUuid(positionals, 3, 'Asset release ID'),
          acl,
          mutation.idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function requireAssetRead(arguments_: ParsedArguments, arities: number[], usage: string): void {
  if (!arities.includes(arguments_.positionals.length)) {
    throw usageError(`usage: a3s-cloud ${usage}`);
  }
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
}

function assetKind(value: string | undefined): AssetKind {
  if (value === 'agent' || value === 'mcp' || value === 'skill') {
    return value;
  }
  throw usageError('Asset kind must be agent, mcp, or skill');
}

function releaseVersion(value: string | undefined): string {
  if (!value || value.length > 128 || /[\0\r\n]/.test(value)) {
    throw usageError('Asset release version must be a bounded semantic version');
  }
  return value;
}

function commitSha(value: string | undefined): string {
  if (!value || !/^(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})$/.test(value)) {
    throw usageError('Git commit SHA must be a full 40- or 64-character hexadecimal ID');
  }
  return value.toLowerCase();
}
