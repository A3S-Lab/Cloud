import type { CloudApi } from '../../lib/api';
import type { BuildRunLogsPage, WorkloadLogStreamFilter } from '../../types/api';
import {
  consumeLogRecordStream,
  isLogRecord,
  isNullableString,
  isRecord,
  isSafeSequence,
  type LogStreamResult,
  useLogStream,
} from './use-log-stream';

export async function consumeBuildRunLogStream(
  response: Response,
  onRecords: (records: BuildRunLogsPage['records']) => void,
  onEventId: (eventId: string) => void
): Promise<void> {
  await consumeLogRecordStream(response, 'build log stream', parseBuildRunLogsPage, onRecords, onEventId);
}

export function useBuildRunLogStream(
  api: CloudApi | null,
  organizationId: string | null,
  buildRunId: string | null,
  stream?: WorkloadLogStreamFilter
): LogStreamResult {
  const url =
    api && organizationId && buildRunId ? api.buildRunLogStreamUrl(organizationId, buildRunId, stream) : null;
  return useLogStream(api, url, 'build log stream', parseBuildRunLogsPage);
}

function parseBuildRunLogsPage(data: string): BuildRunLogsPage {
  const value: unknown = JSON.parse(data);
  if (
    !isRecord(value) ||
    typeof value.buildRunId !== 'string' ||
    typeof value.operationId !== 'string' ||
    !isSafeSequence(value.generation) ||
    !Array.isArray(value.records) ||
    !isNullableString(value.nextCursor)
  ) {
    throw new Error('build log stream returned an invalid page');
  }
  for (const record of value.records) {
    if (!isLogRecord(record)) {
      throw new Error('build log stream returned an invalid record');
    }
  }
  return value as unknown as BuildRunLogsPage;
}
