import { Braces, RotateCcw, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { ServiceTemplate, Workload } from '../../types/api';
import { formatTimestamp } from './console-format';
import {
  diffTemplates,
  eligibleRollbackRevisions,
  isWorkloadReadyForReplacement,
  parseServiceTemplateDraft,
} from './workload-view-model';

interface WorkloadActionsProps {
  workload: Workload;
  onUpdate: (template: ServiceTemplate, idempotencyKey: string) => Promise<void>;
  onRollback: (revisionId: string, idempotencyKey: string) => Promise<void>;
}

interface UpdateDialogState {
  sourceRevisionId: string;
  sourceGeneration: number;
  sourceTemplate: ServiceTemplate;
  draft: string;
  idempotencyKey: string;
}

interface RollbackDialogState {
  revisionId: string;
  idempotencyKey: string;
}

export function WorkloadActions({ workload, onUpdate, onRollback }: WorkloadActionsProps) {
  const [updateDialog, setUpdateDialog] = useState<UpdateDialogState | null>(null);
  const [rollbackDialog, setRollbackDialog] = useState<RollbackDialogState | null>(null);
  const [submitting, setSubmitting] = useState<'update' | 'rollback' | null>(null);
  const ready = isWorkloadReadyForReplacement(workload);
  const rollbackRevisions = eligibleRollbackRevisions(workload);
  const currentRevision = workload.desiredRevision;
  const parsedDraft = updateDialog
    ? parseServiceTemplateDraft(updateDialog.draft)
    : { template: null, error: null };
  const changes =
    updateDialog && parsedDraft.template
      ? diffTemplates(updateDialog.sourceTemplate, parsedDraft.template)
      : [];
  const updateProjectionChanged =
    updateDialog !== null && currentRevision?.id !== updateDialog.sourceRevisionId;
  const selectedRollback = rollbackDialog
    ? rollbackRevisions.find((revision) => revision.id === rollbackDialog.revisionId)
    : undefined;

  const openUpdate = () => {
    if (!ready || !currentRevision) return;
    setUpdateDialog({
      sourceRevisionId: currentRevision.id,
      sourceGeneration: currentRevision.generation,
      sourceTemplate: structuredClone(currentRevision.requestedTemplate),
      draft: JSON.stringify(currentRevision.requestedTemplate, null, 2),
      idempotencyKey: mutationKey('update', workload.id),
    });
  };

  const openRollback = () => {
    if (!ready || rollbackRevisions.length === 0) return;
    setRollbackDialog({
      revisionId: rollbackRevisions[0].id,
      idempotencyKey: mutationKey('rollback', workload.id),
    });
  };

  const submitUpdate = async () => {
    if (
      !updateDialog ||
      !parsedDraft.template ||
      parsedDraft.error ||
      changes.length === 0 ||
      updateProjectionChanged
    ) {
      return;
    }
    setSubmitting('update');
    try {
      await onUpdate(parsedDraft.template, updateDialog.idempotencyKey);
      setUpdateDialog(null);
    } catch {
      // The console owns the shared error banner; keep this draft and key retryable.
    } finally {
      setSubmitting(null);
    }
  };

  const submitRollback = async () => {
    if (!rollbackDialog || !selectedRollback || !ready) return;
    setSubmitting('rollback');
    try {
      await onRollback(selectedRollback.id, rollbackDialog.idempotencyKey);
      setRollbackDialog(null);
    } catch {
      // The console owns the shared error banner; keep this selection and key retryable.
    } finally {
      setSubmitting(null);
    }
  };

  return (
    <>
      <div className='workload-actions'>
        <button
          className='secondary-button compact'
          type='button'
          disabled={!ready}
          title={ready ? 'Commit a complete immutable replacement' : replacementUnavailable}
          onClick={openUpdate}
        >
          <Braces size={14} /> Update
        </button>
        <button
          className='secondary-button compact'
          type='button'
          disabled={!ready || rollbackRevisions.length === 0}
          title={
            rollbackRevisions.length === 0
              ? 'No older successfully activated revision is eligible'
              : ready
                ? 'Clone an older activated revision into a new generation'
                : replacementUnavailable
          }
          onClick={openRollback}
        >
          <RotateCcw size={14} /> Roll back
        </button>
      </div>

      {updateDialog ? (
        <DialogFrame
          labelId='update-workload-title'
          closeDisabled={submitting === 'update'}
          onClose={() => setUpdateDialog(null)}
        >
          <div className='action-dialog-heading'>
            <div>
              <p className='eyebrow'>Immutable replacement</p>
              <h2 id='update-workload-title'>Update {workload.name}</h2>
            </div>
            <CloseButton disabled={submitting === 'update'} onClick={() => setUpdateDialog(null)} />
          </div>
          <p className='action-dialog-intro'>
            Edit the complete requested template for generation {updateDialog.sourceGeneration}. Secret values
            are never projected here; bindings contain references only.
          </p>
          <label className='template-editor'>
            <span>Complete Service template</span>
            <textarea
              value={updateDialog.draft}
              spellCheck={false}
              onChange={(event) =>
                setUpdateDialog((current) => (current ? { ...current, draft: event.target.value } : current))
              }
            />
          </label>
          {parsedDraft.error ? <p className='dialog-validation'>{parsedDraft.error}</p> : null}
          {updateProjectionChanged ? (
            <p className='dialog-validation'>
              The authoritative desired revision changed while this editor was open. Close and reopen it
              before submitting.
            </p>
          ) : null}
          <div className='template-diff'>
            <div className='template-diff-heading'>
              <strong>Field-level changes</strong>
              <span>{changes.length}</span>
            </div>
            {changes.length === 0 ? (
              <p>No template fields have changed.</p>
            ) : (
              <ol>
                {changes.map((change) => (
                  <li key={change.path}>
                    <code>{change.path}</code>
                    <span>
                      <del>{change.before}</del>
                      <ins>{change.after}</ins>
                    </span>
                  </li>
                ))}
              </ol>
            )}
          </div>
          <div className='action-dialog-footer'>
            <span>A single idempotency key is retained while this dialog stays open.</span>
            <button
              className='primary-action'
              type='button'
              disabled={
                submitting === 'update' ||
                parsedDraft.template === null ||
                changes.length === 0 ||
                updateProjectionChanged
              }
              onClick={submitUpdate}
            >
              {submitting === 'update' ? 'Committing…' : 'Commit replacement'}
            </button>
          </div>
        </DialogFrame>
      ) : null}

      {rollbackDialog ? (
        <DialogFrame
          labelId='rollback-workload-title'
          closeDisabled={submitting === 'rollback'}
          onClose={() => setRollbackDialog(null)}
        >
          <div className='action-dialog-heading'>
            <div>
              <p className='eyebrow'>Manual rollback</p>
              <h2 id='rollback-workload-title'>Roll back {workload.name}</h2>
            </div>
            <CloseButton disabled={submitting === 'rollback'} onClick={() => setRollbackDialog(null)} />
          </div>
          <p className='action-dialog-intro'>
            Select an older successfully activated revision. Cloud clones its exact resolved template into a
            new generation and uses the normal health, cutover, and retirement path.
          </p>
          <div className='rollback-options' role='radiogroup' aria-label='Rollback source revision'>
            {rollbackRevisions.map((revision) => {
              const deployment = workload.deployments.find(
                (item) => item.revision.id === revision.id && item.activatedAt
              );
              return (
                <label key={revision.id}>
                  <input
                    type='radio'
                    name='rollback-revision'
                    value={revision.id}
                    checked={revision.id === rollbackDialog.revisionId}
                    onChange={() =>
                      setRollbackDialog((current) =>
                        current ? { ...current, revisionId: revision.id } : current
                      )
                    }
                  />
                  <span>
                    <strong>Generation {revision.generation}</strong>
                    <small>{revision.artifactUri ?? revision.artifactSourceUri}</small>
                    <small>Activated {formatTimestamp(deployment?.activatedAt ?? null)}</small>
                  </span>
                </label>
              );
            })}
          </div>
          <div className='action-dialog-footer'>
            <span>The source revision ID is recorded on the durable operation.</span>
            <button
              className='primary-action'
              type='button'
              disabled={submitting === 'rollback' || !selectedRollback || !ready}
              onClick={submitRollback}
            >
              {submitting === 'rollback'
                ? 'Committing…'
                : `Roll back to generation ${selectedRollback?.generation ?? ''}`}
            </button>
          </div>
        </DialogFrame>
      ) : null}
    </>
  );
}

function DialogFrame({
  labelId,
  closeDisabled,
  onClose,
  children,
}: {
  labelId: string;
  closeDisabled: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const dialogRef = useRef<HTMLElement>(null);
  const onCloseRef = useRef(onClose);
  const closeDisabledRef = useRef(closeDisabled);
  onCloseRef.current = onClose;
  closeDisabledRef.current = closeDisabled;

  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const root = document.getElementById('root');
    const rootWasInert = root?.hasAttribute('inert') ?? false;
    const previousBodyOverflow = document.body.style.overflow;
    root?.setAttribute('inert', '');
    document.body.style.overflow = 'hidden';
    dialogRef.current?.focus();

    const keepFocusInDialog = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !closeDisabledRef.current) {
        onCloseRef.current();
        return;
      }
      if (event.key !== 'Tab' || !dialogRef.current) return;
      const focusable = [...dialogRef.current.querySelectorAll<HTMLElement>(focusableSelector)].filter(
        (element) => !element.hasAttribute('disabled') && element.getAttribute('aria-hidden') !== 'true'
      );
      if (focusable.length === 0) {
        event.preventDefault();
        dialogRef.current.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable.at(-1);
      if (
        event.shiftKey &&
        (document.activeElement === dialogRef.current ||
          document.activeElement === first ||
          !dialogRef.current.contains(document.activeElement))
      ) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener('keydown', keepFocusInDialog);
    return () => {
      window.removeEventListener('keydown', keepFocusInDialog);
      if (!rootWasInert) root?.removeAttribute('inert');
      document.body.style.overflow = previousBodyOverflow;
      previousFocus?.focus();
    };
  }, []);

  return createPortal(
    <div className='action-dialog-backdrop'>
      <section
        className='action-dialog'
        ref={dialogRef}
        role='dialog'
        aria-modal='true'
        aria-labelledby={labelId}
        tabIndex={-1}
      >
        {children}
      </section>
    </div>,
    document.body
  );
}

function CloseButton({ disabled, onClick }: { disabled: boolean; onClick: () => void }) {
  return (
    <button
      className='icon-button'
      type='button'
      disabled={disabled}
      aria-label='Close dialog'
      onClick={onClick}
    >
      <X size={17} />
    </button>
  );
}

function mutationKey(action: 'update' | 'rollback', workloadId: string): string {
  return `web-${action}:${workloadId}:${crypto.randomUUID()}`;
}

const replacementUnavailable =
  'Update and rollback unlock after the desired revision is the active deployment';

const focusableSelector = 'a[href], button, input, select, textarea, [tabindex]:not([tabindex="-1"])';
