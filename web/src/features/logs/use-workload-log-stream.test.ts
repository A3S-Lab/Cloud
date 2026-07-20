import { describe, expect, it, vi } from 'vitest';
import type { WorkloadLogRecord, WorkloadLogsPage } from '../../types/api';
import { appendBoundedLogRecords, consumeWorkloadLogStream } from './use-workload-log-stream';

describe('workload log stream', () => {
  it('consumes resumable record events split across network chunks', async () => {
    const page: WorkloadLogsPage = {
      workloadId: 'workload-1',
      revisionId: 'revision-1',
      nodeId: 'node-1',
      unitId: 'service-1',
      generation: 1,
      records: [record(7, 'hello')],
      nextCursor: 'v1:7',
    };
    const encoder = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('id: v1:7\nevent: rec'));
        controller.enqueue(encoder.encode(`ords\ndata: ${JSON.stringify(page)}\n\n`));
        controller.close();
      },
    });
    const onRecords = vi.fn();
    const onEventId = vi.fn();

    await consumeWorkloadLogStream(new Response(body, { status: 200 }), onRecords, onEventId);

    expect(onEventId).toHaveBeenCalledWith('v1:7');
    expect(onRecords).toHaveBeenCalledWith(page.records);
  });

  it('deduplicates replayed sequences and bounds browser memory', () => {
    const replay = record(2, 'replacement');
    const merged = appendBoundedLogRecords(
      [record(1, 'one'), record(2, 'old')],
      [replay, record(3, 'three'), record(4, 'four')],
      3
    );

    expect(merged).toEqual([replay, record(3, 'three'), record(4, 'four')]);
    expect(appendBoundedLogRecords(merged, [], 0)).toEqual([]);
  });

  it('rejects unsafe numeric cursors from a malformed stream', async () => {
    const page = {
      workloadId: 'workload-1',
      revisionId: 'revision-1',
      nodeId: null,
      unitId: 'service-1',
      generation: 1,
      records: [{ ...record(1, 'hello'), sequence: Number.MAX_SAFE_INTEGER + 1 }],
      nextCursor: 'v1:9007199254740992',
    };
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(
          new TextEncoder().encode(
            `id: v1:9007199254740992\nevent: records\ndata: ${JSON.stringify(page)}\n\n`
          )
        );
        controller.close();
      },
    });

    await expect(
      consumeWorkloadLogStream(new Response(body, { status: 200 }), vi.fn(), vi.fn())
    ).rejects.toThrow('invalid record');
  });
});

function record(sequence: number, data: string): WorkloadLogRecord {
  return {
    kind: 'data',
    sourceCursor: `cursor:${sequence}`,
    sequence,
    observedAtMs: sequence,
    stream: 'stdout',
    data,
    gapReason: null,
    fromSequence: null,
    throughSequence: null,
    compactedChunks: null,
  };
}
