import { CircleAlert, Radio, SquareTerminal } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import type { WorkloadLogRecord, WorkloadLogStreamFilter } from '../../types/api';
import { MAX_VISIBLE_LOG_RECORDS, useWorkloadLogStream } from './use-workload-log-stream';

type LogFilter = 'all' | WorkloadLogStreamFilter;

interface LiveLogPanelProps {
  api: CloudApi;
  organizationId: string | null;
  workloadId: string | null;
  revisionId: string | null;
  generation: number | null;
}

export function LiveLogPanel({ api, organizationId, workloadId, revisionId, generation }: LiveLogPanelProps) {
  const [filter, setFilter] = useState<LogFilter>('all');
  const viewport = useRef<HTMLDivElement>(null);
  const { records, state, error } = useWorkloadLogStream(
    api,
    organizationId,
    workloadId,
    revisionId,
    filter === 'all' ? undefined : filter
  );
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
    <section className='surface live-log-panel' aria-label='Live workload logs'>
      <div className='live-log-heading'>
        <div>
          <p className='eyebrow'>Bounded live delivery</p>
          <h2>
            <SquareTerminal size={19} /> Workload logs
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
                disabled={!revisionId}
                onClick={() => setFilter(value)}
              >
                {value}
              </button>
            ))}
          </fieldset>
        </div>
      </div>
      <div className='live-log-meta'>
        <span>{generation ? `Generation ${generation}` : 'No active revision'}</span>
        <span>Showing the latest {MAX_VISIBLE_LOG_RECORDS} ordered records at most</span>
      </div>
      <div className='live-log-viewport' ref={viewport} role='log' aria-live='polite'>
        {!revisionId ? (
          <div className='live-log-empty'>
            <SquareTerminal size={22} />
            <span>Logs become available after a revision is scheduled.</span>
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

function streamLabel(state: 'idle' | 'connecting' | 'live' | 'retrying'): string {
  if (state === 'live') return 'Live';
  if (state === 'retrying') return 'Reconnecting';
  if (state === 'connecting') return 'Connecting';
  return 'Idle';
}
