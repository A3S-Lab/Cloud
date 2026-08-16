import {
  type CloudApi,
  MAX_DURABLE_CELL_APPLICATION_ACL_BYTES,
  MAX_DURABLE_CELL_SERVICE_PROFILE_ACL_BYTES,
  MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES,
  MAX_WORKLOAD_ACL_BYTES,
} from '@a3s/cloud-client';
import {
  readAclDocument,
  requireAclFilePath,
  requireAclMutationCommand,
  requireVersionedAclMutationCommand,
} from './acl-file';
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
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import {
  durableCellApplicationMutationResult,
  durableCellApplicationRecordResult,
  durableCellApplicationRevisionResult,
  durableCellApplicationRevisionsResult,
  durableCellApplicationsResult,
  durableCellDeploymentResult,
  durableCellRoutePublicationResult,
} from './durable-cell-results';
import { canonicalHostname, canonicalRoutePath } from './edge-commands';
import { usageError } from './errors';
import type { CommandResult } from './results';

interface DurableCellCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export function rejectMisplacedDurableCellOptions(command: string, arguments_: ParsedArguments): void {
  const deployment = command === 'durable-cell-deployments create';
  const route = command === 'durable-cell-routes publish';
  if (arguments_.serviceProfileFile !== undefined && !deployment && !route) {
    throw usageError('--service-profile-file is valid only for Durable Cell deployment or route publication');
  }
  if (
    (arguments_.providerWorkloadFile !== undefined || arguments_.storageBindingFile !== undefined) &&
    !deployment
  ) {
    throw usageError(
      '--provider-workload-file and --storage-binding-file are valid only for Durable Cell deployment'
    );
  }
}

export async function executeDurableCellCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: DurableCellCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  const environmentId = () => requireEnvironment(context);
  switch (command) {
    case 'durable-cell-applications list':
      requireListCommand(arguments_);
      return durableCellApplicationsResult(
        await cloudApi().listDurableCellApplications(organizationId(), projectId(), environmentId())
      );
    case 'durable-cell-applications get':
      requireReadCommand(arguments_, 'durable-cell-applications get <application-id>');
      return durableCellApplicationRecordResult(
        await cloudApi().getDurableCellApplication(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Durable Cell application ID')
        )
      );
    case 'durable-cell-applications create': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'durable-cell-applications create <name>');
      const definitionAcl = await readAclDocument(
        mutation.file,
        {
          label: 'Durable Cell application ACL',
          maximumBytes: MAX_DURABLE_CELL_APPLICATION_ACL_BYTES,
        },
        dependencies.readFile
      );
      return durableCellApplicationMutationResult(
        await cloudApi().createDurableCellApplication(
          organizationId(),
          projectId(),
          environmentId(),
          { name: positionalResourceName(positionals, 2), definitionAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'durable-cell-applications revise': {
      const mutation = requireVersionedAclMutationCommand(
        arguments_,
        3,
        'durable-cell-applications revise <application-id>',
        'Durable Cell application'
      );
      const definitionAcl = await readAclDocument(
        mutation.file,
        {
          label: 'Durable Cell application ACL',
          maximumBytes: MAX_DURABLE_CELL_APPLICATION_ACL_BYTES,
        },
        dependencies.readFile
      );
      return durableCellApplicationMutationResult(
        await cloudApi().reviseDurableCellApplication(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Durable Cell application ID'),
          { expectedVersion: mutation.expectedVersion, definitionAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'durable-cell-applications start':
    case 'durable-cell-applications stop': {
      const action = command.endsWith(' start') ? 'start' : 'stop';
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        `durable-cell-applications ${action} <application-id>`,
        'Durable Cell application'
      );
      const applicationId = positionalUuid(positionals, 2, 'Durable Cell application ID');
      const api = cloudApi();
      const result =
        action === 'start'
          ? await api.startDurableCellApplication(
              organizationId(),
              projectId(),
              environmentId(),
              applicationId,
              mutation.expectedVersion,
              mutation.idempotencyKey
            )
          : await api.stopDurableCellApplication(
              organizationId(),
              projectId(),
              environmentId(),
              applicationId,
              mutation.expectedVersion,
              mutation.idempotencyKey
            );
      return durableCellApplicationMutationResult(result);
    }
    case 'durable-cell-revisions list':
      requireReadCommand(arguments_, 'durable-cell-revisions list <application-id>');
      return durableCellApplicationRevisionsResult(
        await cloudApi().listDurableCellApplicationRevisions(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Durable Cell application ID')
        )
      );
    case 'durable-cell-revisions get':
      requireReadCommand(arguments_, 'durable-cell-revisions get <application-id> <revision-id>', 4);
      return durableCellApplicationRevisionResult(
        await cloudApi().getDurableCellApplicationRevision(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Durable Cell application ID'),
          positionalUuid(positionals, 3, 'Durable Cell application revision ID')
        )
      );
    case 'durable-cell-deployments create': {
      const mutation = requireDeploymentMutation(arguments_);
      const [serviceProfileAcl, providerWorkloadAcl, storageBindingAcl] = await Promise.all([
        readAclDocument(
          mutation.serviceProfileFile,
          {
            label: 'Durable Cell Service-profile ACL',
            maximumBytes: MAX_DURABLE_CELL_SERVICE_PROFILE_ACL_BYTES,
          },
          dependencies.readFile
        ),
        readAclDocument(
          mutation.providerWorkloadFile,
          { label: 'Durable Cell provider Workload ACL', maximumBytes: MAX_WORKLOAD_ACL_BYTES },
          dependencies.readFile
        ),
        readAclDocument(
          mutation.storageBindingFile,
          {
            label: 'Durable Cell storage-binding ACL',
            maximumBytes: MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES,
          },
          dependencies.readFile
        ),
      ]);
      return durableCellDeploymentResult(
        await cloudApi().deployDurableCellApplication(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Durable Cell application ID'),
          positionalUuid(positionals, 3, 'Durable Cell application revision ID'),
          { serviceProfileAcl, providerWorkloadAcl, storageBindingAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'durable-cell-routes publish': {
      const mutation = requireRouteMutation(arguments_);
      const serviceProfileAcl = await readAclDocument(
        mutation.serviceProfileFile,
        {
          label: 'Durable Cell Service-profile ACL',
          maximumBytes: MAX_DURABLE_CELL_SERVICE_PROFILE_ACL_BYTES,
        },
        dependencies.readFile
      );
      return durableCellRoutePublicationResult(
        await cloudApi().publishDurableCellApplicationRoute(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Durable Cell application ID'),
          positionalUuid(positionals, 3, 'Durable Cell application revision ID'),
          {
            serviceProfileAcl,
            gatewayScopeId: positionalUuid(positionals, 4, 'Gateway scope ID'),
            domainClaimId: positionalUuid(positionals, 5, 'domain claim ID'),
            hostname: canonicalHostname(positionals[6], 'route hostname'),
            pathPrefix: canonicalRoutePath(positionals[7]),
          },
          mutation.idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function requireDeploymentMutation(arguments_: ParsedArguments): {
  idempotencyKey: string;
  serviceProfileFile: string;
  providerWorkloadFile: string;
  storageBindingFile: string;
} {
  requireArity(arguments_.positionals, 4, 'durable-cell-deployments create <application-id> <revision-id>');
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  return {
    idempotencyKey: requireIdempotencyKey(arguments_),
    serviceProfileFile: requireAclFilePath(arguments_.serviceProfileFile, '--service-profile-file'),
    providerWorkloadFile: requireAclFilePath(arguments_.providerWorkloadFile, '--provider-workload-file'),
    storageBindingFile: requireAclFilePath(arguments_.storageBindingFile, '--storage-binding-file'),
  };
}

function requireRouteMutation(arguments_: ParsedArguments): {
  idempotencyKey: string;
  serviceProfileFile: string;
} {
  requireArity(
    arguments_.positionals,
    8,
    'durable-cell-routes publish <application-id> <revision-id> <gateway-scope-id> <domain-claim-id> <hostname> <path-prefix>'
  );
  rejectLogOptions(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  return {
    idempotencyKey: requireIdempotencyKey(arguments_),
    serviceProfileFile: requireAclFilePath(arguments_.serviceProfileFile, '--service-profile-file'),
  };
}
