export type CloudProcessRole = 'all' | 'api' | 'worker' | 'relay';

export interface CloudPlatformInfo {
  name: string;
  version: string;
  role: CloudProcessRole;
}

export type CloudHealthStatus = 'up' | 'down';

export interface CloudHealthIndicator {
  status: CloudHealthStatus;
  details: unknown;
}

export interface CloudHealthReport {
  status: CloudHealthStatus;
  checks: Record<string, CloudHealthIndicator>;
}

export interface CloudDiagnostics {
  platform: CloudPlatformInfo;
  liveness: CloudHealthReport;
  readiness: CloudHealthReport;
}
