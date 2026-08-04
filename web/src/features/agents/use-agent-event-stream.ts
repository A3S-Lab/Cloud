import { useEffect, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import { consumeSseStream, type StreamState, waitForStreamRetry } from '../../lib/sse';
import type { AgentExecutionEvent, AgentExecutionEventsPage } from '../../types/api';

export const MAX_VISIBLE_AGENT_EVENTS = 500;

export interface AgentEventStreamResult {
  records: AgentExecutionEvent[];
  state: StreamState;
  error: string | null;
}

export function appendBoundedAgentEvents(
  current: AgentExecutionEvent[],
  incoming: AgentExecutionEvent[],
  limit = MAX_VISIBLE_AGENT_EVENTS
): AgentExecutionEvent[] {
  if (limit <= 0) return [];
  const records = new Map(current.map((record) => [record.sequence, record]));
  for (const record of incoming) records.set(record.sequence, record);
  return [...records.values()].sort((left, right) => left.sequence - right.sequence).slice(-limit);
}

export async function consumeAgentEventStream(
  response: Response,
  onRecords: (records: AgentExecutionEvent[]) => void,
  onEventId: (eventId: string) => void
): Promise<void> {
  await consumeSseStream(response, 'Agent event stream', (event) => {
    if (event.event !== 'records' || !event.data) return;
    const page = parseAgentEventPage(event.data);
    if (event.id) onEventId(event.id);
    onRecords(page.records);
  });
}

export function useAgentEventStream(
  api: CloudApi | null,
  organizationId: string | null,
  conversationId: string | null
): AgentEventStreamResult {
  const [records, setRecords] = useState<AgentExecutionEvent[]>([]);
  const [state, setState] = useState<StreamState>('idle');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setRecords([]);
    setError(null);
    if (!api || !organizationId || !conversationId) {
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
          const response = await fetch(api.agentExecutionEventStreamUrl(organizationId, conversationId), {
            headers: api.eventStreamHeaders(lastEventId),
            signal: controller.signal,
          });
          if (response.status === 401 || response.status === 403) {
            throw new Error('Agent event stream authorization failed');
          }
          setState('live');
          setError(null);
          attempt = 0;
          await consumeAgentEventStream(
            response,
            (incoming) => setRecords((current) => appendBoundedAgentEvents(current, incoming)),
            (eventId) => {
              lastEventId = eventId;
            }
          );
          throw new Error('Agent event stream closed');
        } catch (cause) {
          if (controller.signal.aborted) return;
          const message = cause instanceof Error ? cause.message : 'Agent event stream failed';
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
  }, [api, conversationId, organizationId]);

  return { records, state, error };
}

function parseAgentEventPage(data: string): AgentExecutionEventsPage {
  const value: unknown = JSON.parse(data);
  if (
    !isRecord(value) ||
    typeof value.conversationId !== 'string' ||
    !isSafeSequence(value.headSequence) ||
    !Array.isArray(value.records) ||
    !isNullableString(value.nextCursor)
  ) {
    throw new Error('Agent event stream returned an invalid page');
  }
  for (const record of value.records) {
    if (!isAgentEvent(record)) throw new Error('Agent event stream returned an invalid record');
  }
  return value as unknown as AgentExecutionEventsPage;
}

function isAgentEvent(value: unknown): value is AgentExecutionEvent {
  return (
    isRecord(value) &&
    typeof value.organizationId === 'string' &&
    typeof value.conversationId === 'string' &&
    typeof value.executionId === 'string' &&
    isSafeSequence(value.sequence) &&
    isAgentEventKind(value.kind) &&
    typeof value.contentDigest === 'string' &&
    isSafeSequence(value.contentSizeBytes) &&
    typeof value.occurredAt === 'string'
  );
}

function isAgentEventKind(value: unknown): boolean {
  return (
    value === 'execution_requested' ||
    value === 'model_output' ||
    value === 'execution_failed' ||
    value === 'execution_completed'
  );
}

function isSafeSequence(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === 'string';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
