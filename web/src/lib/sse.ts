export type StreamState = 'idle' | 'connecting' | 'live' | 'retrying';

export interface ParsedSseEvent {
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

export async function consumeSseStream(
  response: Response,
  streamName: string,
  onEvent: (event: ParsedSseEvent) => void
): Promise<void> {
  if (!response.ok || !response.body) {
    throw new Error(`${streamName} returned HTTP ${response.status}`);
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
      if (event) {
        onEvent(event);
      }
    }
    if (done) {
      return;
    }
  }
}

export function waitForStreamRetry(signal: AbortSignal, attempt: number): Promise<void> {
  const delay = Math.min(1_000 * 2 ** Math.min(Math.max(attempt, 1) - 1, 4), 15_000);
  return new Promise<void>((resolve) => {
    const timeout = window.setTimeout(resolve, delay);
    signal.addEventListener(
      'abort',
      () => {
        window.clearTimeout(timeout);
        resolve();
      },
      { once: true }
    );
  });
}
