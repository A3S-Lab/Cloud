import { type CloudApi, MAX_EXECUTION_TEMPLATE_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import { positionalUuid, requireListCommand, requireReadCommand } from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import {
  executionTemplateMutationResult,
  executionTemplateResult,
  executionTemplatesResult,
} from './execution-template-results';
import type { CommandResult } from './results';

interface ExecutionTemplateCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeExecutionTemplateCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: ExecutionTemplateCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'execution-templates list':
      requireListCommand(arguments_);
      return executionTemplatesResult(
        await cloudApi().listExecutionTemplates(requireOrganization(context), requireProject(context))
      );
    case 'execution-templates get':
      requireReadCommand(arguments_, 'execution-templates get <template-id> <revision-id>', 4);
      return executionTemplateResult(
        await cloudApi().getExecutionTemplate(
          requireOrganization(context),
          requireProject(context),
          positionalUuid(positionals, 2, 'ExecutionTemplate ID'),
          positionalUuid(positionals, 3, 'ExecutionTemplate revision ID')
        )
      );
    case 'execution-templates create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'execution-templates create');
      const definitionAcl = await readAclDocument(
        mutation.file,
        {
          label: 'ExecutionTemplate ACL',
          maximumBytes: MAX_EXECUTION_TEMPLATE_ACL_BYTES,
        },
        dependencies.readFile
      );
      return executionTemplateMutationResult(
        await cloudApi().createExecutionTemplate(
          requireOrganization(context),
          requireProject(context),
          { definitionAcl },
          mutation.idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}
