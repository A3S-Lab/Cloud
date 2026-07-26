import {
  CloudApi,
  type CloudFetch,
  type Environment,
  type Node,
  type Operation,
  type Organization,
  type Project,
} from '@a3s/cloud-client';
import type { CloudContext, PublicCloudContext } from './context';
import { publicContext, requireOrganization, requireProject, requireToken } from './context';
import { usageError } from './errors';
import { renderTable, type TableColumn } from './output';

export interface CommandResult {
  json: unknown;
  table: string;
}

export async function executeCommand(
  positionals: readonly string[],
  context: CloudContext,
  fetcher?: CloudFetch
): Promise<CommandResult> {
  if (positionals.length !== 2) {
    throw usageError('a command and action are required; run a3s-cloud --help');
  }
  const command = `${positionals[0]} ${positionals[1]}`;
  if (command === 'context show') {
    return contextResult(publicContext(context));
  }

  const api = new CloudApi(requireToken(context), context.baseUrl, {
    fetch: fetcher,
    requestTimeoutMs: context.timeoutMs,
  });
  switch (command) {
    case 'organizations list':
      return organizationsResult(await api.listOrganizations());
    case 'projects list':
      return projectsResult(await api.listProjects(requireOrganization(context)));
    case 'environments list':
      return environmentsResult(
        await api.listEnvironments(requireOrganization(context), requireProject(context))
      );
    case 'nodes list':
      return nodesResult(await api.listNodes(requireOrganization(context)));
    case 'operations list':
      return operationsResult(await api.listOperations(requireOrganization(context)));
    default:
      throw usageError('unsupported command; run a3s-cloud --help');
  }
}

function contextResult(context: PublicCloudContext): CommandResult {
  const rows = [
    { key: 'URL', value: context.url },
    { key: 'Organization', value: context.organizationId ?? '' },
    { key: 'Project', value: context.projectId ?? '' },
    { key: 'Environment', value: context.environmentId ?? '' },
    { key: 'Output', value: context.output },
    { key: 'Timeout (ms)', value: context.timeoutMs },
    { key: 'Token configured', value: context.tokenConfigured ? 'yes' : 'no' },
  ];
  return {
    json: context,
    table: renderTable(rows, [
      { header: 'CONTEXT', value: (row) => row.key },
      { header: 'VALUE', value: (row) => row.value },
    ]),
  };
}

function organizationsResult(rows: Organization[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'VERSION', value: (row) => row.aggregateVersion },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ]);
}

function projectsResult(rows: Project[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'VERSION', value: (row) => row.aggregateVersion },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ]);
}

function environmentsResult(rows: Environment[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'VERSION', value: (row) => row.aggregateVersion },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ]);
}

function nodesResult(rows: Node[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'STATE', value: (row) => row.state },
    { header: 'AVAILABILITY', value: (row) => row.availability },
    { header: 'PROVIDER', value: (row) => row.runtimeProviderId },
    { header: 'LAST OBSERVED', value: (row) => row.lastObservedAt },
  ]);
}

function operationsResult(rows: Operation[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'SUBJECT', value: (row) => `${row.subjectKind}/${row.subjectId}` },
    { header: 'WORKFLOW', value: (row) => `${row.workflowName}@${row.workflowVersion}` },
    { header: 'STATUS', value: (row) => row.status },
    { header: 'UPDATED AT', value: (row) => row.updatedAt },
    { header: 'ERROR', value: (row) => row.error },
  ]);
}

function listResult<Row>(rows: Row[], columns: readonly TableColumn<Row>[]): CommandResult {
  return {
    json: rows,
    table: renderTable(rows, columns),
  };
}
