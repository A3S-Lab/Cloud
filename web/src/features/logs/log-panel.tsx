import { CircleAlert, Radio, SquareTerminal } from 'lucide-react';
import { useEffect, useRef } from 'react';
import type { StreamState } from '../../lib/sse';
import type { WorkloadLogRecord, WorkloadLogStreamFilter } from '../../types/api';
import { MAX_VISIBLE_LOG_RECORDS } from './use-log-stream';

type LogFilter = 'all' | WorkloadLogStreamFilter;

interface LogPanelProps {
  ariaLabel: string;
  eyebrow: string;
  title: string;
  available: boolean;
  contextLabel: string;
  unavailableMessage: string;
  records: WorkloadLogRecord[];
  state: StreamState;
  error: string | null;
  filter: LogFilter;
  onFilterChange: (filter: LogFilter) => void;
}

export function LogPanel({
  ariaLabel,
  eyebrow,
  title,
  available,
  contextLabel,
  unavailableMessage,
  records,
  state,
  error,
  filter,
  onFilterChange,
}: LogPanelProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const recordCount = records.length;

  useEffect(() => {
    if (recordCount === 0) {
      return;
    }
    const element = viewport.current;
    if (element) {
      element.scrollTop = element.scrollHeight;
    }
  }, [recordCount]);

  return (
    <section className='surface live-log-panel' aria-label={ariaLabel}>
      <div className='live-log-heading'>
        <div>
          <p className='eyebrow'>{eyebrow}</p>
          <h2>
            <SquareTerminal size={19} /> {title}
          </h2>
        </div>
        <div className='live-log-toolbar'>
          <span className={`log-stream-state ${state}`}>
            <Radio size={13} />
            {streamLabel(state)}
          </span>
          <fieldset className='log-filter'>
            <legend className='sr-only'>Log stream filter</legend>
            {(['all', 'stdout', 'stderr'] as const).map((value) => (
              <button
                className={filter === value ? 'selected' : ''}
                type='button'
                key={value}
                disabled={!available}
                onClick={() => onFilterChange(value)}
              >
                {value}
              </button>
            ))}
          </fieldset>
        </div>
      </div>
      <div className='live-log-meta'>
        <span>{contextLabel}</span>
        <span>Showing the latest {MAX_VISIBLE_LOG_RECORDS} ordered records at most</span>
      </div>
      <div className='live-log-viewport' ref={viewport} role='log' aria-live='polite'>
        {!available ? (
          <div className='live-log-empty'>
            <SquareTerminal size={22} />
            <span>{unavailableMessage}</span>
          </div>
        ) : records.length === 0 ? (
          <div className='live-log-empty'>
            <Radio size={22} />
            <span>
              {state === 'live'
                ? 'Connected. Waiting for ordered log records.'
                : 'Connecting to the authoritative log stream.'}
            </span>
          </div>
        ) : (
          records.map((record) => <LogRecord record={record} key={record.sequence} />)
        )}
      </div>
      {error ? (
        <output className='live-log-error'>
          <CircleAlert size={14} />
          {error}
        </output>
      ) : null}
    </section>
  );
}

function LogRecord({ record }: { record: WorkloadLogRecord }) {
  if (record.kind === 'gap') {
    return (
      <div className='live-log-gap'>
        <span>{sequenceLabel(record)}</span>
        <strong>{gapLabel(record)}</strong>
      </div>
    );
  }
  return (
    <div className={`live-log-record ${record.stream ?? 'unknown'}`}>
      <span className='live-log-sequence'>#{record.sequence}</span>
      <time>{timestampLabel(record.observedAtMs)}</time>
      <span className='live-log-stream'>{record.stream ?? 'unknown'}</span>
      <pre>{record.data ?? ''}</pre>
    </div>
  );
}

function sequenceLabel(record: WorkloadLogRecord): string {
  if (record.fromSequence !== null && record.throughSequence !== null) {
    return `#${record.fromSequence}–${record.throughSequence}`;
  }
  return `#${record.sequence}`;
}

function gapLabel(record: WorkloadLogRecord): string {
  const reason = (record.gapReason ?? 'unknown').replaceAll('_', ' ');
  if (record.compactedChunks !== null) {
    return `${reason} · ${record.compactedChunks} records`;
  }
  return reason;
}

function timestampLabel(value: number | null): string {
  if (value === null) {
    return 'unknown time';
  }
  const observedAt = new Date(value);
  if (Number.isNaN(observedAt.getTime())) {
    return `${value} ms`;
  }
  return new Intl.DateTimeFormat(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    fractionalSecondDigits: 3,
  }).format(observedAt);
}

function streamLabel(state: StreamState): string {
  if (state === 'live') return 'Live';
  if (state === 'retrying') return 'Reconnecting';
  if (state === 'connecting') return 'Connecting';
  return 'Idle';
}
