import { Activity, CheckCheck } from 'lucide-react';
import type { Operation } from '../../types/api';
import type { StreamState } from '../operations/use-operation-stream';
import { formatRelative, humanize, shortId, streamLabel } from './console-format';
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
  const visible = visibleOperations(operations, dismissedOperationIds);
  const terminalIds = visible.filter(isTerminalOperation).map((operation) => operation.id);

  return (
    <aside className='operation-drawer' aria-label='Operations'>
      <div className='drawer-heading'>
        <div>
          <p className='eyebrow'>Durable timeline</p>
          <h2>Operations</h2>
        </div>
        <output className={`stream-dot ${streamState}`} aria-label={streamLabel(streamState)} />
      </div>
      {terminalIds.length > 0 ? (
        <button className='drawer-cleanup' type='button' onClick={() => onDismissTerminal(terminalIds)}>
          <CheckCheck size={14} />
          Clear {terminalIds.length} terminal
        </button>
      ) : null}
      <div className='operation-list'>
        {visible.length === 0 ? (
          <div className='empty-operations'>
            <Activity size={22} />
            <strong>No visible operations</strong>
            <p>Active work and new authoritative terminal results will appear here.</p>
          </div>
        ) : (
          visible.map((operation) => (
            <article className='operation-item' key={operation.id}>
              <span className={`operation-status ${operation.status}`} />
              <div>
                <div className='operation-title'>
                  <strong>{humanize(operation.subjectKind)}</strong>
                  <span>{operation.status}</span>
                </div>
                <p>
                  {operation.workflowName}@{operation.workflowVersion}
                </p>
                {operation.rollbackSourceRevisionId ? (
                  <small>rollback source {shortId(operation.rollbackSourceRevisionId)}</small>
                ) : null}
                {operation.externalSourceRevisionId ? (
                  <small>
                    source {shortId(operation.externalSourceRevisionId)}
                    {operation.buildRunId ? ` · build ${shortId(operation.buildRunId)}` : ''}
                  </small>
                ) : null}
                {!operation.externalSourceRevisionId && operation.buildRunId ? (
                  <small>build {shortId(operation.buildRunId)}</small>
                ) : null}
                <small>
                  seq {operation.lastSequence} · {formatRelative(operation.updatedAt)}
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
