import { describe, expect, it } from 'vitest';
import type { AgentExecutionEvent } from '../../types/api';
import { appendBoundedAgentEvents, consumeAgentEventStream } from './use-agent-event-stream';

describe('Agent event streaming', () => {
  it('deduplicates, orders, and bounds semantic events by conversation sequence', () => {
    expect(
      appendBoundedAgentEvents([event(2), event(1)], [event(2, 'model_output'), event(3)], 2).map(
        (record) => [record.sequence, record.kind]
      )
    ).toEqual([
      [2, 'model_output'],
      [3, 'execution_requested'],
    ]);
    expect(appendBoundedAgentEvents([event(1)], [event(2)], 0)).toEqual([]);
  });

  it('consumes shared records events and advances the opaque resume cursor', async () => {
    const encoder = new TextEncoder();
    const page = {
      conversationId: 'conversation',
      headSequence: 2,
      records: [event(2)],
      nextCursor: 'opaque-2',
    };
    const payload = `id: opaque-2\nevent: records\ndata: ${JSON.stringify(page)}\n\n`;
    const response = new Response(
      new ReadableStream({
        start(controller) {
          controller.enqueue(encoder.encode(payload.slice(0, 19)));
          controller.enqueue(encoder.encode(payload.slice(19)));
          controller.close();
        },
      })
    );
    const records: AgentExecutionEvent[][] = [];
    const cursors: string[] = [];

    await consumeAgentEventStream(
      response,
      (incoming) => records.push(incoming),
      (cursor) => cursors.push(cursor)
    );

    expect(records).toEqual([[event(2)]]);
    expect(cursors).toEqual(['opaque-2']);
  });

  it('rejects malformed event pages before exposing records', async () => {
    const response = new Response(
      `event: records\ndata: ${JSON.stringify({
        conversationId: 'conversation',
        headSequence: 1,
        records: [{ ...event(1), kind: 'tool_request' }],
        nextCursor: null,
      })}\n\n`
    );

    await expect(
      consumeAgentEventStream(
        response,
        () => undefined,
        () => undefined
      )
    ).rejects.toThrow('invalid record');
  });
});

function event(
  sequence: number,
  kind: AgentExecutionEvent['kind'] = 'execution_requested'
): AgentExecutionEvent {
  return {
    organizationId: 'organization',
    conversationId: 'conversation',
    executionId: 'execution',
    sequence,
    kind,
    content: { sequence },
    contentDigest: `sha256:${'a'.repeat(64)}`,
    contentSizeBytes: 16,
    occurredAt: '2026-08-04T00:00:00.000Z',
  };
}
