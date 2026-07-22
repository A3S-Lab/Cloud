import type { CloudApi } from '../../lib/api';
import type { WorkloadLogsPage, WorkloadLogStreamFilter } from '../../types/api';
import {
  appendBoundedLogRecords,
  consumeLogRecordStream,
  isLogRecord,
  isNullableString,
  isRecord,
  isSafeSequence,
  type LogStreamResult,
  MAX_VISIBLE_LOG_RECORDS,
  useLogStream,
} from './use-log-stream';

export { appendBoundedLogRecords, MAX_VISIBLE_LOG_RECORDS };

export async function consumeWorkloadLogStream(
  response: Response,
  onRecords: (records: WorkloadLogsPage['records']) => void,
  onEventId: (eventId: string) => void
): Promise<void> {
  await consumeLogRecordStream(response, 'workload log stream', parseWorkloadLogsPage, onRecords, onEventId);
}

export function useWorkloadLogStream(
  api: CloudApi | null,
  organizationId: string | null,
  workloadId: string | null,
  revisionId: string | null,
  stream?: WorkloadLogStreamFilter
): LogStreamResult {
  const url =
    api && organizationId && workloadId && revisionId
      ? api.workloadLogStreamUrl(organizationId, workloadId, revisionId, stream)
      : null;
  return useLogStream(api, url, 'workload log stream', parseWorkloadLogsPage);
}

function parseWorkloadLogsPage(data: string): WorkloadLogsPage {
  const value: unknown = JSON.parse(data);
  if (
    !isRecord(value) ||
    typeof value.workloadId !== 'string' ||
    typeof value.revisionId !== 'string' ||
    !isNullableString(value.nodeId) ||
    typeof value.unitId !== 'string' ||
    !isSafeSequence(value.generation) ||
    !Array.isArray(value.records) ||
    !isNullableString(value.nextCursor)
  ) {
    throw new Error('workload log stream returned an invalid page');
  }
  for (const record of value.records) {
    if (!isLogRecord(record)) {
      throw new Error('workload log stream returned an invalid record');
    }
  }
  return value as unknown as WorkloadLogsPage;
}
