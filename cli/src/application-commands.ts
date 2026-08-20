import { type CloudApi, MAX_APPLICATION_RELEASE_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand, requireVersionedAclMutationCommand } from './acl-file';
import {
  applicationMutationResult,
  applicationResult,
  applicationReleaseResult,
  applicationReleasesResult,
  applicationsResult,
} from './application-results';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
  positionalUuid,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import type { CommandResult } from './results';

interface ApplicationCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeApplicationCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: ApplicationCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  switch (command) {
    case 'applications list':
      requireListCommand(arguments_);
      return applicationsResult(await cloudApi().listApplications(organizationId(), projectId()));
    case 'applications get':
      requireReadCommand(arguments_, 'applications get <application-id>');
      return applicationResult(
        await cloudApi().getApplication(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID')
        )
      );
    case 'applications create': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'applications create <name>');
      const releaseAcl = await readApplicationAcl(mutation.file, dependencies.readFile);
      return applicationMutationResult(
        await cloudApi().createApplication(
          organizationId(),
          projectId(),
          {
            name: positionalResourceName(positionals, 2),
            description: '',
            releaseAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'applications publish': {
      const mutation = requireVersionedAclMutationCommand(
        arguments_,
        3,
        'applications publish <application-id>',
        'Application'
      );
      const releaseAcl = await readApplicationAcl(mutation.file, dependencies.readFile);
      return applicationMutationResult(
        await cloudApi().publishApplicationRelease(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          { expectedVersion: mutation.expectedVersion, releaseAcl },
          mutation.idempotencyKey
        )
      );
    }
    case 'application-releases list':
      requireReadCommand(arguments_, 'application-releases list <application-id>');
      return applicationReleasesResult(
        await cloudApi().listApplicationReleases(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID')
        )
      );
    case 'application-releases get':
      requireReadCommand(arguments_, 'application-releases get <application-id> <release-id>', 4);
      return applicationReleaseResult(
        await cloudApi().getApplicationRelease(
          organizationId(),
          projectId(),
          positionalUuid(positionals, 2, 'Application ID'),
          positionalUuid(positionals, 3, 'Application release ID')
        )
      );
    default:
      return undefined;
  }
}

function readApplicationAcl(
  file: string,
  readFile: ((path: string) => Promise<Uint8Array>) | undefined
): Promise<string> {
  return readAclDocument(
    file,
    {
      label: 'Application release ACL',
      maximumBytes: MAX_APPLICATION_RELEASE_ACL_BYTES,
    },
    readFile
  );
}
