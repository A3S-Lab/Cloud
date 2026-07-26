export class CloudApiError extends Error {
  readonly status: number;
  readonly statusCode: string;
  readonly requestId?: string;
  readonly details: Readonly<Record<string, unknown>>;

  constructor(
    status: number,
    message: string,
    statusCode = 'HTTP_ERROR',
    requestId?: string,
    details: Record<string, unknown> = {}
  ) {
    super(message);
    this.name = 'CloudApiError';
    this.status = status;
    this.statusCode = statusCode;
    this.requestId = requestId;
    this.details = Object.freeze(details);
  }
}
