import { useEffect, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import { consumeSseStream, parseSseEvent, type StreamState, waitForStreamRetry } from '../../lib/sse';
import type { Operation } from '../../types/api';

export { parseSseEvent };
export type { StreamState };

export async function consumeOperationStream(
  response: Response,
  onSnapshot: (operations: Operation[]) => void,
  onEventId: (eventId: string) => void
): Promise<void> {
  await consumeSseStream(response, 'operation stream', (event) => {
    if (event.event === 'snapshot' && event.data) {
      onSnapshot(JSON.parse(event.data) as Operation[]);
      if (event.id) {
        onEventId(event.id);
      }
    }
  });
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
          throw new Error('operation stream closed');
        } catch (error) {
          if (controller.signal.aborted) {
            return;
          }
          if (error instanceof Error && error.message.includes('authorization failed')) {
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
  }, [api, organizationId, onSnapshot]);

  return state;
}
