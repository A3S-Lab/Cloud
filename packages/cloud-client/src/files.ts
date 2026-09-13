export const USER_FILE_ADMISSION_CONTRACT_SCHEMA = 'cloud.user-file.v1' as const;
export const USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES = 64 * 1024;
export const USER_FILE_MAX_BYTES = 512 * 1024 * 1024;
export const DEFAULT_USER_FILE_LIST_LIMIT = 50;
export const MAXIMUM_USER_FILE_LIST_LIMIT = 200;

export type UserFileState =
  | 'awaiting_upload'
  | 'awaiting_scan'
  | 'admitted'
  | 'rejected'
  | 'expired'
  | 'tombstoned';

export interface UserFile {
  organizationId: string;
  projectId: string;
  userFileId: string;
  uploadId: string;
  state: UserFileState;
  originalName: string;
  contractSchema: typeof USER_FILE_ADMISSION_CONTRACT_SCHEMA;
  admissionAcl: string;
  contractDigest: string;
  objectRef: string;
  contentDigest: string;
  sizeBytes: number;
  mediaType: string;
  scanPolicy: 'required';
  uploadExpiresAt: string;
  retentionUntil: string;
  scanEvidenceDigest: string | null;
  rejectionReasonCode: string | null;
  tombstonedFrom: Exclude<UserFileState, 'tombstoned'> | null;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  uploadedAt: string | null;
  scannedAt: string | null;
  expiredAt: string | null;
  tombstonedAt: string | null;
  cleanupDueAt: string | null;
  updatedAt: string;
}

export interface ReserveUserFileInput {
  admissionAcl: string;
}

export type UserFileScanDecision =
  | { kind: 'admitted' }
  | { kind: 'rejected'; reasonCode: string };

export interface RecordUserFileScanInput {
  expectedVersion: number;
  evidenceDigest: string;
  decision: UserFileScanDecision;
}

export interface UserFileMutationResult {
  file: UserFile;
  replayed: boolean;
}

export interface UserFileQuota {
  organizationId: string;
  limitBytes: number;
  allocatedBytes: number;
  availableBytes: number;
  revision: number;
  updatedAt: string | null;
}

export interface UserFileListOptions {
  limit?: number;
}

export function validateUserFileAdmissionAcl(value: unknown): asserts value is string {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    new TextEncoder().encode(value).byteLength > USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES ||
    /\r(?!\n)/.test(value)
  ) {
    throw new TypeError(
      `UserFile admission ACL must be a bounded canonical .acl document of at most ${USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES} bytes`
    );
  }
}

export function validateExpectedUserFileVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('UserFile expected version must be a positive safe integer');
  }
}

const USER_FILE_EVIDENCE_DIGEST_PATTERN = /^sha256:[0-9a-f]{64}$/;
const USER_FILE_REJECTION_REASON_PATTERN = /^[a-z0-9._-]{1,64}$/;

export function validateUserFileEvidenceDigest(value: unknown): asserts value is string {
  if (typeof value !== 'string' || !USER_FILE_EVIDENCE_DIGEST_PATTERN.test(value)) {
    throw new TypeError('UserFile evidence digest must use canonical sha256:<64 lowercase hex> syntax');
  }
}

export function validateUserFileScanDecision(value: unknown): asserts value is UserFileScanDecision {
  if (!value || typeof value !== 'object') {
    throw new TypeError('UserFile scan decision must be an object');
  }
  const decision = value as Record<string, unknown>;
  if (decision.kind === 'admitted') {
    if (Object.keys(decision).length !== 1) {
      throw new TypeError('UserFile admitted decision must only include kind');
    }
    return;
  }
  if (decision.kind === 'rejected') {
    if (typeof decision.reasonCode !== 'string' || !USER_FILE_REJECTION_REASON_PATTERN.test(decision.reasonCode)) {
      throw new TypeError('UserFile rejected decision requires a bounded lowercase reasonCode');
    }
    if (Object.keys(decision).length !== 2) {
      throw new TypeError('UserFile rejected decision must only include kind and reasonCode');
    }
    return;
  }
  throw new TypeError('UserFile scan decision kind must be admitted or rejected');
}

export function encodeUserFileListOptions(options: UserFileListOptions = {}): string {
  const limit = options.limit ?? DEFAULT_USER_FILE_LIST_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAXIMUM_USER_FILE_LIST_LIMIT) {
    throw new RangeError(`UserFile list limit must be between 1 and ${MAXIMUM_USER_FILE_LIST_LIMIT}`);
  }
  return `?limit=${limit}`;
}
