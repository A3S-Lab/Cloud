import { Activity, CheckCheck } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { Operation } from '../../types/api';
import type { StreamState } from '../operations/use-operation-stream';
import { shortId } from './console-format';
import { isTerminalOperation, visibleOperations } from './workload-view-model';

interface OperationDrawerProps {
  operations: Operation[];
  dismissedOperationIds: ReadonlySet<string>;
  streamState: StreamState;
  onDismissTerminal: (operationIds: string[]) => void;
}

export function OperationDrawer({
  operations,
  dismissedOperationIds,
  streamState,
  onDismissTerminal,
}: OperationDrawerProps) {
  const { formatRelative, label, t } = useI18n();
  const visible = visibleOperations(operations, dismissedOperationIds);
  const terminalIds = visible.filter(isTerminalOperation).map((operation) => operation.id);

  return (
    <aside className='operation-drawer' aria-label={t('Operations')}>
      <div className='drawer-heading'>
        <div>
          <p className='eyebrow'>{t('Durable timeline')}</p>
          <h2>{t('Operations')}</h2>
        </div>
        <output className={`stream-dot ${streamState}`} aria-label={label(streamState)} />
      </div>
      {terminalIds.length > 0 ? (
        <button className='drawer-cleanup' type='button' onClick={() => onDismissTerminal(terminalIds)}>
          <CheckCheck size={14} />
          {t('Clear {count} terminal', { count: terminalIds.length })}
        </button>
      ) : null}
      <div className='operation-list'>
        {visible.length === 0 ? (
          <div className='empty-operations'>
            <Activity size={22} />
            <strong>{t('No visible operations')}</strong>
            <p>{t('Active work and new authoritative terminal results will appear here.')}</p>
          </div>
        ) : (
          visible.map((operation) => (
            <article className='operation-item' key={operation.id}>
              <span className={`operation-status ${operation.status}`} />
              <div>
                <div className='operation-title'>
                  <strong>{label(operation.subjectKind)}</strong>
                  <span>{label(operation.status)}</span>
                </div>
                <p>
                  {operation.workflowName}@{operation.workflowVersion}
                </p>
                {operation.rollbackSourceRevisionId ? (
                  <small>
                    {t('rollback source {source}', {
                      source: shortId(operation.rollbackSourceRevisionId),
                    })}
                  </small>
                ) : null}
                {operation.externalSourceRevisionId ? (
                  <small>
                    {t('source {source}', { source: shortId(operation.externalSourceRevisionId) })}
                    {operation.buildRunId
                      ? ` · ${t('build {build}', { build: shortId(operation.buildRunId) })}`
                      : ''}
                  </small>
                ) : null}
                {!operation.externalSourceRevisionId && operation.buildRunId ? (
                  <small>{t('build {build}', { build: shortId(operation.buildRunId) })}</small>
                ) : null}
                <small>
                  {t('seq {sequence} · {time}', {
                    sequence: operation.lastSequence,
                    time: formatRelative(operation.updatedAt),
                  })}
                </small>
                {operation.error ? <em>{operation.error}</em> : null}
              </div>
            </article>
          ))
        )}
      </div>
    </aside>
  );
}
