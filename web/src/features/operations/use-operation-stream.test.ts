import { describe, expect, it, vi } from 'vitest';
import type { Operation } from '../../types/api';
import { consumeOperationStream, parseSseEvent } from './use-operation-stream';

const operation: Operation = {
  id: 'operation-1',
  organizationId: 'organization-1',
  subjectKind: 'deployment',
  subjectId: 'deployment-1',
  workflowName: 'cloud.deployment',
  workflowVersion: '1',
  status: 'running',
  lastSequence: 2,
  requestedAt: '2026-07-14T00:00:00Z',
  updatedAt: '2026-07-14T00:00:01Z',
  error: null,
};

describe('operation stream', () => {
  it('parses named snapshots and ignores heartbeat comments', () => {
    expect(parseSseEvent(': keepalive')).toBeNull();
    expect(parseSseEvent('id: 7\nevent: snapshot\ndata: [{"id":"1"}]')).toEqual({
      id: '7',
      event: 'snapshot',
      data: '[{"id":"1"}]',
    });
  });

  it('handles events split across network chunks', async () => {
    const encoder = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('id: snapshot-1\nevent: snap'));
        controller.enqueue(encoder.encode(`shot\ndata: ${JSON.stringify([operation])}\n\n`));
        controller.close();
      },
    });
    const onSnapshot = vi.fn();
    const onEventId = vi.fn();

    await consumeOperationStream(new Response(body, { status: 200 }), onSnapshot, onEventId);

    expect(onSnapshot).toHaveBeenCalledWith([operation]);
    expect(onEventId).toHaveBeenCalledWith('snapshot-1');
  });
});
