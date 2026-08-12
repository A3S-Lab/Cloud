import type { ExecutionTemplateMutationResult, ExecutionTemplateRevision } from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const EXECUTION_TEMPLATE_COLUMNS = [
  { header: 'TEMPLATE', value: (row: ExecutionTemplateRevision) => row.templateId },
  { header: 'REVISION', value: (row: ExecutionTemplateRevision) => row.revisionId },
  { header: 'DIGEST', value: (row: ExecutionTemplateRevision) => row.definitionDigest },
  { header: 'CAPABILITY', value: (row: ExecutionTemplateRevision) => row.capability },
  { header: 'CREATED AT', value: (row: ExecutionTemplateRevision) => row.createdAt },
] as const;

export function executionTemplatesResult(rows: ExecutionTemplateRevision[]): CommandResult {
  return { json: rows, table: renderTable(rows, EXECUTION_TEMPLATE_COLUMNS) };
}

export function executionTemplateResult(row: ExecutionTemplateRevision): CommandResult {
  return { json: row, table: renderTable([row], EXECUTION_TEMPLATE_COLUMNS) };
}

export function executionTemplateMutationResult(row: ExecutionTemplateMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [{ ...row.executionTemplate, replayed: row.replayed }],
      [...EXECUTION_TEMPLATE_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}
