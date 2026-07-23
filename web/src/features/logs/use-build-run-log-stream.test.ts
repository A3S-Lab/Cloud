import { describe, expect, it, vi } from 'vitest';
import type { BuildRunLogsPage, WorkloadLogRecord } from '../../types/api';
import { consumeBuildRunLogStream } from './use-build-run-log-stream';

describe('build log stream', () => {
  it('consumes resumable records without requiring private node identity', async () => {
    const page: BuildRunLogsPage = {
      buildRunId: 'build-1',
      operationId: 'operation-1',
      generation: 1,
      records: [record(7, 'building')],
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

    await consumeBuildRunLogStream(new Response(body, { status: 200 }), onRecords, onEventId);

    expect(onEventId).toHaveBeenCalledWith('v1:7');
    expect(onRecords).toHaveBeenCalledWith(page.records);
  });

  it('rejects unsafe numeric sequences from a malformed stream', async () => {
    const page = {
      buildRunId: 'build-1',
      operationId: 'operation-1',
      generation: 1,
      records: [{ ...record(1, 'building'), sequence: Number.MAX_SAFE_INTEGER + 1 }],
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
      consumeBuildRunLogStream(new Response(body, { status: 200 }), vi.fn(), vi.fn())
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
