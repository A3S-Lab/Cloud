import { type CloudApi, MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand, requireVersionedAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
  positionalUuid,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import {
  connectorProfileMutationResult,
  connectorProfileRecordResult,
  connectorProfilesResult,
  connectorRevisionResult,
  connectorRevisionsResult,
} from './connector-results';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import type { CommandResult } from './results';

interface ConnectorCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeConnectorCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: ConnectorCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  const environmentId = () => requireEnvironment(context);
  switch (command) {
    case 'connector-profiles list':
      requireListCommand(arguments_);
      return connectorProfilesResult(
        await cloudApi().listConnectorProfiles(organizationId(), projectId(), environmentId())
      );
    case 'connector-profiles get':
      requireReadCommand(arguments_, 'connector-profiles get <profile-id>');
      return connectorProfileRecordResult(
        await cloudApi().getConnectorProfile(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Connector profile ID')
        )
      );
    case 'connector-profiles create': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'connector-profiles create <name>');
      const definitionAcl = await readConnectorAcl(mutation.file, dependencies.readFile);
      return connectorProfileMutationResult(
        await cloudApi().createConnectorProfile(
          organizationId(),
          projectId(),
          environmentId(),
          {
            name: positionalResourceName(positionals, 2),
            definitionAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'connector-profiles revise': {
      const mutation = requireVersionedAclMutationCommand(
        arguments_,
        3,
        'connector-profiles revise <profile-id>',
        'Connector profile'
      );
      const definitionAcl = await readConnectorAcl(mutation.file, dependencies.readFile);
      return connectorProfileMutationResult(
        await cloudApi().reviseConnectorProfile(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Connector profile ID'),
          { expectedVersion: mutation.expectedVersion, definitionAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'connector-revisions list':
      requireReadCommand(arguments_, 'connector-revisions list <profile-id>');
      return connectorRevisionsResult(
        await cloudApi().listConnectorRevisions(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Connector profile ID')
        )
      );
    case 'connector-revisions get':
      requireReadCommand(arguments_, 'connector-revisions get <profile-id> <revision-id>', 4);
      return connectorRevisionResult(
        await cloudApi().getConnectorRevision(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Connector profile ID'),
          positionalUuid(positionals, 3, 'Connector revision ID')
        )
      );
    default:
      return undefined;
  }
}

function readConnectorAcl(
  file: string,
  readFile: ((path: string) => Promise<Uint8Array>) | undefined
): Promise<string> {
  return readAclDocument(
    file,
    {
      label: 'Connector definition ACL',
      maximumBytes: MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES,
    },
    readFile
  );
}
