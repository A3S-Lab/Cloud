export interface Ontology {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  description: string;
  currentRevisionId: string;
  currentRevisionNumber: number;
  currentRevisionDigest: string;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
}

export type OntologyMigrationPolicyKind = 'initial' | 'compatible' | 'explicit';

export interface OntologyMigrationPolicy {
  kind: OntologyMigrationPolicyKind;
  ruleId: string | null;
  expressionDigest: string | null;
}

export interface OntologyRevisionSummary {
  organizationId: string;
  projectId: string;
  ontologyId: string;
  id: string;
  revisionNumber: number;
  parentRevisionId: string | null;
  parentDigest: string | null;
  contractSchema: 'cloud.workflow.ontology.v1';
  compilerSchemaVersion: number;
  contentDigest: string;
  migrationPolicy: OntologyMigrationPolicy;
  createdBy: string;
  createdAt: string;
}

export interface OntologyRevision extends OntologyRevisionSummary {
  canonicalAcl: string;
}

export type OntologyResourceKind = 'metadata' | 'object_type' | 'relation_type' | 'rule';
export type OntologyChangeKind = 'added' | 'removed' | 'changed';
export type OntologyChangeCompatibility = 'compatible' | 'breaking';

export interface OntologyChange {
  resourceKind: OntologyResourceKind;
  resourceId: string;
  changeKind: OntologyChangeKind;
  compatibility: OntologyChangeCompatibility;
  changedFields: string[];
}

export interface OntologyDiff {
  ontologyId: string;
  fromRevisionId: string;
  toRevisionId: string;
  fromDigest: string;
  toDigest: string;
  breaking: boolean;
  changes: OntologyChange[];
}

export interface OntologyMutationResult {
  ontology: Ontology;
  revision: OntologyRevision;
  diff: Omit<OntologyDiff, 'ontologyId' | 'fromRevisionId' | 'toRevisionId'> | null;
  replayed: boolean;
}

export interface ReviseOntologyOptions {
  expectedVersion: number;
  migrationRuleId?: string;
}
