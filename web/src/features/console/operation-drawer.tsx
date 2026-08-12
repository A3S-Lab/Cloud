import { Activity, CheckCheck } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { Operation } from '../../types/api';
import type { StreamState } from '../operations/use-operation-stream';
import { shortId, statusBadgeState } from './console-format';
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
    <aside className='task-pane operation-drawer' data-responsive='overlay' aria-label={t('Operations')}>
      <header className='drawer-heading'>
        <div>
          <h2>{t('Operations')}</h2>
          <p>{t('Durable timeline')}</p>
        </div>
        <output
          className='status-badge'
          data-state={statusBadgeState(streamState)}
          data-size='sm'
          data-indicator
        >
          {label(streamState)}
        </output>
      </header>
      {terminalIds.length > 0 ? (
        <button
          className='btn drawer-cleanup'
          data-size='xs'
          data-variant='outline'
          type='button'
          onClick={() => onDismissTerminal(terminalIds)}
        >
          <CheckCheck size={14} />
          {t('Clear {count} terminal', { count: terminalIds.length })}
        </button>
      ) : null}
      <section className='item-group operation-list'>
        {visible.length === 0 ? (
          <div className='empty empty-operations'>
            <figure>
              <Activity size={22} />
            </figure>
            <header>
              <h3>{t('No visible operations')}</h3>
              <p>{t('Active work and new authoritative terminal results will appear here.')}</p>
            </header>
          </div>
        ) : (
          visible.map((operation) => (
            <article className='item operation-item' data-size='sm' data-variant='outline' key={operation.id}>
              <span className={`operation-status ${operation.status}`} />
              <section>
                <div className='operation-title'>
                  <strong>{label(operation.subjectKind)}</strong>
                  <span
                    className='status-badge'
                    data-state={statusBadgeState(operation.status)}
                    data-size='sm'
                    data-indicator
                  >
                    {label(operation.status)}
                  </span>
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
              </section>
            </article>
          ))
        )}
      </section>
    </aside>
  );
}
