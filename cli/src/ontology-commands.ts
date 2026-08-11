import { type CloudApi, MAX_ONTOLOGY_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import {
  ontologiesResult,
  ontologyDiffResult,
  ontologyMutationResult,
  ontologyResult,
  ontologyRevisionResult,
  ontologyRevisionsResult,
} from './ontology-results';
import type { CommandResult } from './results';

interface OntologyCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeOntologyCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: OntologyCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'ontologies list':
      rejectMigrationRule(arguments_);
      requireListCommand(arguments_);
      return ontologiesResult(
        await cloudApi().listOntologies(requireOrganization(context), requireProject(context))
      );
    case 'ontologies get':
      rejectMigrationRule(arguments_);
      requireReadCommand(arguments_, 'ontologies get <ontology-id>');
      return ontologyResult(
        await cloudApi().getOntology(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Ontology ID')
        )
      );
    case 'ontologies create': {
      rejectMigrationRule(arguments_);
      const mutation = requireAclMutationCommand(arguments_, 2, 'ontologies create');
      const acl = await readOntologyAcl(mutation.file, dependencies.readFile);
      return ontologyMutationResult(
        await cloudApi().createOntologyFromAcl(
          requireOrganization(context),
          requireProject(context),
          acl,
          mutation.idempotencyKey
        )
      );
    }
    case 'ontologies revisions':
      rejectMigrationRule(arguments_);
      requireReadCommand(arguments_, 'ontologies revisions <ontology-id>');
      return ontologyRevisionsResult(
        await cloudApi().listOntologyRevisions(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Ontology ID')
        )
      );
    case 'ontologies revision':
      rejectMigrationRule(arguments_);
      requireArity(positionals, 4, 'ontologies revision <ontology-id> <revision-id>');
      rejectReadMutationOptions(arguments_);
      return ontologyRevisionResult(
        await cloudApi().getOntologyRevision(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Ontology ID'),
          positionalUuid(positionals, 3, 'Ontology revision ID')
        )
      );
    case 'ontologies diff':
      rejectMigrationRule(arguments_);
      requireArity(positionals, 5, 'ontologies diff <ontology-id> <from-revision-id> <to-revision-id>');
      rejectReadMutationOptions(arguments_);
      return ontologyDiffResult(
        await cloudApi().diffOntologyRevisions(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Ontology ID'),
          positionalUuid(positionals, 3, 'source Ontology revision ID'),
          positionalUuid(positionals, 4, 'target Ontology revision ID')
        )
      );
    case 'ontologies revise': {
      const mutation = requireRevisionMutation(arguments_);
      const acl = await readOntologyAcl(mutation.file, dependencies.readFile);
      return ontologyMutationResult(
        await cloudApi().reviseOntologyFromAcl(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Ontology ID'),
          acl,
          {
            expectedVersion: mutation.expectedVersion,
            ...(mutation.migrationRuleId === undefined ? {} : { migrationRuleId: mutation.migrationRuleId }),
          },
          mutation.idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function requireRevisionMutation(arguments_: ParsedArguments): {
  expectedVersion: number;
  migrationRuleId?: string;
  idempotencyKey: string;
  file: string;
} {
  requireArity(arguments_.positionals, 3, 'ontologies revise <ontology-id>');
  rejectLogOptions(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const file = arguments_.file;
  if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('--file with a valid A3S ACL path is required for Ontology revision');
  }
  const rawVersion = arguments_.expectedVersion;
  if (rawVersion === undefined || !/^[0-9]+$/.test(rawVersion)) {
    throw usageError('--expected-version must be a positive safe integer for Ontology revision');
  }
  const expectedVersion = Number(rawVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError('--expected-version must be a positive safe integer for Ontology revision');
  }
  const migrationRuleId = arguments_.migrationRuleId;
  if (migrationRuleId !== undefined && !/^[A-Za-z0-9_-]{1,96}$/.test(migrationRuleId)) {
    throw usageError('--migration-rule must be a portable Ontology migration rule ID');
  }
  return { expectedVersion, migrationRuleId, idempotencyKey, file };
}

function rejectReadMutationOptions(arguments_: ParsedArguments): void {
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  if (arguments_.expectedVersion !== undefined) {
    throw usageError('--expected-version is valid only for ontologies revise');
  }
  rejectGatewayRolloutOptions(arguments_);
}

function rejectMigrationRule(arguments_: ParsedArguments): void {
  if (arguments_.migrationRuleId !== undefined) {
    throw usageError('--migration-rule is valid only for ontologies revise');
  }
}

function readOntologyAcl(path: string, readFile?: (path: string) => Promise<Uint8Array>): Promise<string> {
  return readAclDocument(path, { label: 'Ontology ACL', maximumBytes: MAX_ONTOLOGY_ACL_BYTES }, readFile);
}
