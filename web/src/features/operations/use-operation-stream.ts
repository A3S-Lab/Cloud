import { useEffect, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import type { Operation } from '../../types/api';

export type StreamState = 'idle' | 'connecting' | 'live' | 'retrying';

interface ParsedSseEvent {
  id?: string;
  event?: string;
  data?: string;
}

export function parseSseEvent(block: string): ParsedSseEvent | null {
  const event: ParsedSseEvent = {};
  const data: string[] = [];
  for (const rawLine of block.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n')) {
    if (!rawLine || rawLine.startsWith(':')) {
      continue;
    }
    const separator = rawLine.indexOf(':');
    const field = separator === -1 ? rawLine : rawLine.slice(0, separator);
    const value = separator === -1 ? '' : rawLine.slice(separator + 1).replace(/^ /, '');
    if (field === 'id') {
      event.id = value;
    } else if (field === 'event') {
      event.event = value;
    } else if (field === 'data') {
      data.push(value);
    }
  }
  if (data.length > 0) {
    event.data = data.join('\n');
  }
  return Object.keys(event).length > 0 ? event : null;
}

export async function consumeOperationStream(
  response: Response,
  onSnapshot: (operations: Operation[]) => void,
  onEventId: (eventId: string) => void
): Promise<void> {
  if (!response.ok || !response.body) {
    throw new Error(`operation stream returned HTTP ${response.status}`);
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: !done });
    const blocks = buffer.split(/\r?\n\r?\n/);
    buffer = blocks.pop() ?? '';
    for (const block of blocks) {
      const event = parseSseEvent(block);
      if (event?.event === 'snapshot' && event.data) {
        onSnapshot(JSON.parse(event.data) as Operation[]);
        if (event.id) {
          onEventId(event.id);
        }
      }
    }
    if (done) {
      return;
    }
  }
}

export function useOperationStream(
  api: CloudApi | null,
  organizationId: string | null,
  onSnapshot: (operations: Operation[]) => void
): StreamState {
  const [state, setState] = useState<StreamState>('idle');

  useEffect(() => {
    if (!api || !organizationId) {
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
          const response = await fetch(api.operationStreamUrl(organizationId), {
            headers: {
              Accept: 'text/event-stream',
              Authorization: `Bearer ${api.token}`,
              ...(lastEventId ? { 'Last-Event-ID': lastEventId } : {}),
            },
            signal: controller.signal,
          });
          if (response.status === 401 || response.status === 403) {
            throw new Error('operation stream authorization failed');
          }
          setState('live');
          attempt = 0;
          await consumeOperationStream(response, onSnapshot, (eventId) => {
            lastEventId = eventId;
          });
        } catch (error) {
          if (controller.signal.aborted) {
            return;
          }
          attempt += 1;
          setState('retrying');
          const delay = Math.min(1_000 * 2 ** Math.min(attempt - 1, 4), 15_000);
          await new Promise<void>((resolve) => {
            const timeout = window.setTimeout(resolve, delay);
            controller.signal.addEventListener(
              'abort',
              () => {
                window.clearTimeout(timeout);
                resolve();
              },
              { once: true }
            );
          });
          if (error instanceof Error && error.message.includes('authorization failed')) {
            return;
          }
        }
      }
    };

    void run();
    return () => controller.abort();
  }, [api, organizationId, onSnapshot]);

  return state;
}
