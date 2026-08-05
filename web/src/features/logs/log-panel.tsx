import { CircleAlert, Radio, SquareTerminal } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { useI18n } from '../../lib/i18n';
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
  const { label, t } = useI18n();
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
            {label(state)}
          </span>
          <fieldset className='log-filter'>
            <legend className='sr-only'>{t('Log stream filter')}</legend>
            {(['all', 'stdout', 'stderr'] as const).map((value) => (
              <button
                className={filter === value ? 'selected' : ''}
                type='button'
                key={value}
                disabled={!available}
                onClick={() => onFilterChange(value)}
              >
                {value === 'stdout' || value === 'stderr' ? value : label(value)}
              </button>
            ))}
          </fieldset>
        </div>
      </div>
      <div className='live-log-meta'>
        <span>{contextLabel}</span>
        <span>
          {t('Showing the latest {count} ordered records at most', {
            count: MAX_VISIBLE_LOG_RECORDS,
          })}
        </span>
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
                ? t('Connected. Waiting for ordered log records.')
                : t('Connecting to the authoritative log stream.')}
            </span>
          </div>
        ) : (
          records.map((record) => <LogRecord record={record} key={record.sequence} />)
        )}
      </div>
      {error ? (
        <output className='live-log-error'>
          <CircleAlert size={14} />
          {t(error)}
        </output>
      ) : null}
    </section>
  );
}

function LogRecord({ record }: { record: WorkloadLogRecord }) {
  const { language, label, t } = useI18n();
  if (record.kind === 'gap') {
    return (
      <div className='live-log-gap'>
        <span>{sequenceLabel(record)}</span>
        <strong>{gapLabel(record, label, t)}</strong>
      </div>
    );
  }
  return (
    <div className={`live-log-record ${record.stream ?? 'unknown'}`}>
      <span className='live-log-sequence'>#{record.sequence}</span>
      <time>{timestampLabel(record.observedAtMs, language, t)}</time>
      <span className='live-log-stream'>{record.stream ?? 'unknown'}</span>
      <pre>{record.data ?? ''}</pre>
    </div>
  );
}

function sequenceLabel(record: WorkloadLogRecord): string {
  if (record.fromSequence !== null && record.throughSequence !== null) {
    return `#${record.fromSequence} to #${record.throughSequence}`;
  }
  return `#${record.sequence}`;
}

function gapLabel(
  record: WorkloadLogRecord,
  label: (value: string) => string,
  t: (message: string, values?: Record<string, string | number>) => string
): string {
  const reason = label(record.gapReason ?? 'unknown');
  if (record.compactedChunks !== null) {
    return t('{reason} · {count} records', { reason, count: record.compactedChunks });
  }
  return reason;
}

function timestampLabel(
  value: number | null,
  language: 'zh-CN' | 'en',
  t: (message: string) => string
): string {
  if (value === null) {
    return t('unknown time');
  }
  const observedAt = new Date(value);
  if (Number.isNaN(observedAt.getTime())) {
    return `${value} ms`;
  }
  return new Intl.DateTimeFormat(language, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    fractionalSecondDigits: 3,
  }).format(observedAt);
}
