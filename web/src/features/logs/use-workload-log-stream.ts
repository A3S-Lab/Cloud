import { useEffect, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import { consumeSseStream, type StreamState, waitForStreamRetry } from '../../lib/sse';
import type { WorkloadLogRecord, WorkloadLogsPage, WorkloadLogStreamFilter } from '../../types/api';

export const MAX_VISIBLE_LOG_RECORDS = 500;

interface WorkloadLogStreamResult {
  records: WorkloadLogRecord[];
  state: StreamState;
  error: string | null;
}

export function appendBoundedLogRecords(
  current: WorkloadLogRecord[],
  incoming: WorkloadLogRecord[],
  limit = MAX_VISIBLE_LOG_RECORDS
): WorkloadLogRecord[] {
  if (limit <= 0) {
    return [];
  }
  const records = new Map(current.map((record) => [record.sequence, record]));
  for (const record of incoming) {
    records.set(record.sequence, record);
  }
  return [...records.values()].sort((left, right) => left.sequence - right.sequence).slice(-limit);
}

export async function consumeWorkloadLogStream(
  response: Response,
  onRecords: (records: WorkloadLogRecord[]) => void,
  onEventId: (eventId: string) => void
): Promise<void> {
  await consumeSseStream(response, 'workload log stream', (event) => {
    if (event.event !== 'records' || !event.data) {
      return;
    }
    const page = parseWorkloadLogsPage(event.data);
    if (event.id) {
      onEventId(event.id);
    }
    onRecords(page.records);
  });
}

export function useWorkloadLogStream(
  api: CloudApi | null,
  organizationId: string | null,
  workloadId: string | null,
  revisionId: string | null,
  stream?: WorkloadLogStreamFilter
): WorkloadLogStreamResult {
  const [records, setRecords] = useState<WorkloadLogRecord[]>([]);
  const [state, setState] = useState<StreamState>('idle');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setRecords([]);
    setError(null);
    if (!api || !organizationId || !workloadId || !revisionId) {
      setState('idle');
      return;
    }
    const controller = new AbortController();
    let lastEventId = '';

    const run = async () => {
      let attempt = 0;
      while (!controller.signal.aborted) {
        setState(attempt === 0 ? 'connecting' : 'retrying');
        try {
          const response = await fetch(
            api.workloadLogStreamUrl(organizationId, workloadId, revisionId, stream),
            {
              headers: {
                Accept: 'text/event-stream',
                Authorization: `Bearer ${api.token}`,
                ...(lastEventId ? { 'Last-Event-ID': lastEventId } : {}),
              },
              signal: controller.signal,
            }
          );
          if (response.status === 401 || response.status === 403) {
            throw new Error('workload log stream authorization failed');
          }
          setState('live');
          setError(null);
          attempt = 0;
          await consumeWorkloadLogStream(
            response,
            (incoming) => {
              setRecords((current) => appendBoundedLogRecords(current, incoming));
            },
            (eventId) => {
              lastEventId = eventId;
            }
          );
          throw new Error('workload log stream closed');
        } catch (cause) {
          if (controller.signal.aborted) {
            return;
          }
          const message = cause instanceof Error ? cause.message : 'workload log stream failed';
          setError(message);
          if (message.includes('authorization failed')) {
            setState('idle');
            return;
          }
          attempt += 1;
          setState('retrying');
          await waitForStreamRetry(controller.signal, attempt);
        }
      }
    };

    void run();
    return () => controller.abort();
  }, [api, organizationId, revisionId, stream, workloadId]);

  return { records, state, error };
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
    if (!isWorkloadLogRecord(record)) {
      throw new Error('workload log stream returned an invalid record');
    }
  }
  return value as unknown as WorkloadLogsPage;
}

function isWorkloadLogRecord(value: unknown): value is WorkloadLogRecord {
  if (
    !isRecord(value) ||
    (value.kind !== 'data' && value.kind !== 'gap') ||
    !isNullableString(value.sourceCursor) ||
    !isSafeSequence(value.sequence) ||
    !isNullableSafeSequence(value.observedAtMs) ||
    (value.stream !== null && value.stream !== 'stdout' && value.stream !== 'stderr') ||
    !isNullableString(value.data) ||
    !isNullableString(value.gapReason) ||
    !isNullableSafeSequence(value.fromSequence) ||
    !isNullableSafeSequence(value.throughSequence) ||
    !isNullableSafeSequence(value.compactedChunks)
  ) {
    return false;
  }
  return value.kind === 'data'
    ? typeof value.data === 'string' && value.gapReason === null
    : value.data === null && typeof value.gapReason === 'string';
}

function isSafeSequence(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function isNullableSafeSequence(value: unknown): value is number | null {
  return value === null || isSafeSequence(value);
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === 'string';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
