import { CloudApiError } from '@a3s/cloud-client';

export const ExitCode = {
  Success: 0,
  Internal: 1,
  Usage: 2,
  Authentication: 3,
  NotFound: 4,
  Conflict: 5,
  Api: 6,
  Transport: 7,
  Unhealthy: 8,
} as const;

export type ExitCodeValue = (typeof ExitCode)[keyof typeof ExitCode];

export class CliError extends Error {
  readonly exitCode: ExitCodeValue;
  readonly statusCode: string;
  readonly status?: number;
  readonly requestId?: string;
  readonly details: Readonly<Record<string, unknown>>;

  constructor(
    exitCode: ExitCodeValue,
    statusCode: string,
    message: string,
    options: {
      status?: number;
      requestId?: string;
      details?: Record<string, unknown>;
    } = {}
  ) {
    super(message);
    this.name = 'CliError';
    this.exitCode = exitCode;
    this.statusCode = statusCode;
    this.status = options.status;
    this.requestId = options.requestId;
    this.details = Object.freeze(options.details ?? {});
  }
}

export function usageError(message: string): CliError {
  return new CliError(ExitCode.Usage, 'INVALID_ARGUMENT', message);
}

export function normalizeError(error: unknown): CliError {
  if (error instanceof CliError) {
    return error;
  }
  if (error instanceof CloudApiError) {
    return new CliError(apiExitCode(error), error.statusCode, error.message, {
      status: error.status || undefined,
      requestId: error.requestId,
      details: { ...error.details },
    });
  }
  return new CliError(ExitCode.Internal, 'CLI_INTERNAL_ERROR', 'Unexpected CLI failure');
}

function apiExitCode(error: CloudApiError): ExitCodeValue {
  if (error.status === 401 || error.status === 403) {
    return ExitCode.Authentication;
  }
  if (error.status === 404) {
    return ExitCode.NotFound;
  }
  if (error.status === 409) {
    return ExitCode.Conflict;
  }
  if (
    error.status === 0 ||
    ['INVALID_RESPONSE', 'NETWORK_ERROR', 'REQUEST_ABORTED', 'REQUEST_TIMEOUT'].includes(error.statusCode)
  ) {
    return ExitCode.Transport;
  }
  return ExitCode.Api;
}
