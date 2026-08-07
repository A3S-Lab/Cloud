import type { CreateApiTokenInput } from './identity';
import type { IssueEnrollmentTokenInput } from './node';

export const MAX_SECRET_VALUE_BYTES = 1024 * 1024;
export const MAX_ACL_DOCUMENT_BYTES = 64 * 1024;
export const MAX_MCP_SERVICE_PROFILE_ACL_BYTES = MAX_ACL_DOCUMENT_BYTES;
export const MAX_WORKLOAD_ACL_BYTES = MAX_ACL_DOCUMENT_BYTES;

export function validateApiTokenInput(input: CreateApiTokenInput): void {
  if (!/^a3s_[0-9a-f]{64}$/.test(input.token)) {
    throw new TypeError('API token must use the a3s_ prefix followed by 64 lowercase hex digits');
  }
  if (!Array.isArray(input.scopes) || input.scopes.length === 0) {
    throw new TypeError('API token must grant at least one scope');
  }
  const uniqueScopes = new Set<string>();
  for (const scope of input.scopes) {
    if (typeof scope !== 'string' || scope.length > 63 || !/^[a-z-]+:[a-z-]+$/.test(scope)) {
      throw new TypeError('API token scope must use bounded lowercase domain:action syntax');
    }
    if (uniqueScopes.has(scope)) {
      throw new TypeError('API token scopes must be unique');
    }
    uniqueScopes.add(scope);
  }
  if (input.expiresAt !== undefined && input.expiresAt !== null && !isRfc3339Timestamp(input.expiresAt)) {
    throw new TypeError('API token expiry must be an RFC 3339 timestamp');
  }
}

export function validateEnrollmentTokenInput(input: IssueEnrollmentTokenInput): void {
  if (!/^a3sn_[0-9a-f]{64}$/.test(input.token)) {
    throw new TypeError(
      'node enrollment token must use the a3sn_ prefix followed by 64 lowercase hex digits'
    );
  }
  if (
    typeof input.name !== 'string' ||
    input.name.trim() !== input.name ||
    [...input.name].length < 1 ||
    [...input.name].length > 63 ||
    /[\0\r\n]/.test(input.name)
  ) {
    throw new TypeError('node enrollment token name must contain 1 to 63 visible characters');
  }
  if (!isRfc3339Timestamp(input.expiresAt)) {
    throw new TypeError('enrollment credential expiry must be an RFC 3339 timestamp');
  }
}

export function validateExpectedNodeVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected node version must be a positive safe integer');
  }
}

export function validateMcpCredentialExpiry(value: string): void {
  if (!isRfc3339Timestamp(value)) {
    throw new TypeError('MCP credential expiry must be an RFC 3339 timestamp');
  }
}

export function validateExpectedMcpCredentialVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected MCP credential version must be a positive safe integer');
  }
}

export function validateSecretValue(value: string): void {
  const bytes = typeof value === 'string' ? new TextEncoder().encode(value).byteLength : 0;
  if (bytes < 1 || bytes > MAX_SECRET_VALUE_BYTES) {
    throw new RangeError('Secret value must contain between 1 byte and 1 MiB');
  }
}

export function validateWorkloadAcl(manifest: string): void {
  validateAclBytes(manifest, MAX_WORKLOAD_ACL_BYTES, 'workload ACL');
}

export function validateMcpServiceProfileAcl(acl: string): void {
  validateAclBytes(acl, MAX_MCP_SERVICE_PROFILE_ACL_BYTES, 'MCP Service profile ACL');
}

function validateAclBytes(value: string, maximumBytes: number, label: string): void {
  const bytes = typeof value === 'string' ? new TextEncoder().encode(value).byteLength : 0;
  if (bytes < 1 || bytes > maximumBytes) {
    throw new RangeError(`${label} must contain between 1 and ${maximumBytes} UTF-8 bytes`);
  }
}

export function isRfc3339Timestamp(value: string): boolean {
  const match =
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|[+-](\d{2}):(\d{2}))$/.exec(value);
  if (!match || !Number.isFinite(Date.parse(value))) {
    return false;
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offsetHour = match[7] === undefined ? 0 : Number(match[7]);
  const offsetMinute = match[8] === undefined ? 0 : Number(match[8]);
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return (
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= (days[month - 1] ?? 0) &&
    hour <= 23 &&
    minute <= 59 &&
    second <= 59 &&
    offsetHour <= 23 &&
    offsetMinute <= 59
  );
}
