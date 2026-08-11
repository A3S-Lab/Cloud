import type {
  Ontology,
  OntologyDiff,
  OntologyMutationResult,
  OntologyRevision,
  OntologyRevisionSummary,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const ONTOLOGY_COLUMNS = [
  { header: 'ID', value: (row: Ontology) => row.id },
  { header: 'NAME', value: (row: Ontology) => row.name },
  { header: 'REVISION', value: (row: Ontology) => row.currentRevisionNumber },
  { header: 'DIGEST', value: (row: Ontology) => row.currentRevisionDigest },
  { header: 'VERSION', value: (row: Ontology) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row: Ontology) => row.updatedAt },
] as const;

export function ontologiesResult(rows: Ontology[]): CommandResult {
  return { json: rows, table: renderTable(rows, ONTOLOGY_COLUMNS) };
}

export function ontologyResult(row: Ontology): CommandResult {
  return { json: row, table: renderTable([row], ONTOLOGY_COLUMNS) };
}

export function ontologyMutationResult(row: OntologyMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.ontology.id },
        { header: 'NAME', value: (value) => value.ontology.name },
        { header: 'REVISION', value: (value) => value.revision.revisionNumber },
        { header: 'DIGEST', value: (value) => value.revision.contentDigest },
        { header: 'MIGRATION', value: (value) => value.revision.migrationPolicy.kind },
        { header: 'BREAKING', value: (value) => value.diff?.breaking ?? false },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

const REVISION_COLUMNS = [
  { header: 'ID', value: (row: OntologyRevisionSummary) => row.id },
  { header: 'NUMBER', value: (row: OntologyRevisionSummary) => row.revisionNumber },
  { header: 'DIGEST', value: (row: OntologyRevisionSummary) => row.contentDigest },
  { header: 'MIGRATION', value: (row: OntologyRevisionSummary) => row.migrationPolicy.kind },
  { header: 'PARENT', value: (row: OntologyRevisionSummary) => row.parentRevisionId },
  { header: 'CREATED AT', value: (row: OntologyRevisionSummary) => row.createdAt },
] as const;

export function ontologyRevisionsResult(rows: OntologyRevisionSummary[]): CommandResult {
  return { json: rows, table: renderTable(rows, REVISION_COLUMNS) };
}

export function ontologyRevisionResult(row: OntologyRevision): CommandResult {
  return { json: row, table: renderTable([row], REVISION_COLUMNS) };
}

export function ontologyDiffResult(row: OntologyDiff): CommandResult {
  return {
    json: row,
    table: renderTable(row.changes, [
      { header: 'RESOURCE', value: (change) => `${change.resourceKind}/${change.resourceId}` },
      { header: 'CHANGE', value: (change) => change.changeKind },
      { header: 'COMPATIBILITY', value: (change) => change.compatibility },
      { header: 'FIELDS', value: (change) => change.changedFields.join(',') },
    ]),
  };
}
