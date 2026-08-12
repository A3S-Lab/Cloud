export interface FormDraftInput {
  name: string;
  description?: string;
  document: Record<string, unknown>;
}

export type FormCanonicalValue =
  | null
  | boolean
  | number
  | string
  | readonly FormCanonicalValue[]
  | { readonly [key: string]: FormCanonicalValue };

export interface ReviseFormDraftOptions {
  expectedVersion: number;
}

export interface PublishFormReleaseOptions {
  expectedVersion: number;
}

export interface FormReleaseSummary {
  id: string;
  revision: number;
  sourceDraftVersion: number;
  digest: string;
  publishedAt: string;
}

export interface FormDraft {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  description: string;
  document: Record<string, unknown>;
  draftDigest: string;
  aggregateVersion: number;
  latestRelease: FormReleaseSummary | null;
  createdBy: string;
  updatedBy: string;
  createdAt: string;
  updatedAt: string;
}

export interface FormReleaseRef {
  apiVersion: 'a3s.dev/form-release-ref/v1';
  organizationId: string;
  projectId: string;
  formId: string;
  releaseId: string;
  uri: string;
  revision: number;
  digest: string;
  compilerRevision: string;
  schemaProfile: string;
  mode: 'interaction';
}

export interface FormRelease {
  organizationId: string;
  projectId: string;
  formId: string;
  id: string;
  revision: number;
  sourceDraftVersion: number;
  name: string;
  description: string;
  normalizedDocument: Record<string, unknown>;
  formPlan: Record<string, unknown>;
  compilerRevision: string;
  schemaProfile: string;
  contentDigest: string;
  releaseRef: FormReleaseRef;
  publishedBy: string;
  publishedAt: string;
}

export interface FormDraftMutationResult {
  form: FormDraft;
  replayed: boolean;
}

export interface FormPublicationMutationResult {
  form: FormDraft;
  release: FormRelease;
  replayed: boolean;
}
