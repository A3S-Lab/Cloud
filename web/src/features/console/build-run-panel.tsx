import { Ban, Boxes, Hammer, RotateCcw, ShieldCheck, SquareTerminal } from 'lucide-react';
import type { BuildRun, BuildRunStatus } from '../../types/api';
import { compactDigest, formatRelative, humanize, shortId } from './console-format';

interface BuildRunPanelProps {
  buildRuns: BuildRun[];
  selectedBuildRunId: string | null;
  cancellingBuildRunId: string | null;
  retryingBuildRunId: string | null;
  onSelect: (buildRunId: string) => void;
  onCancel: (buildRunId: string) => void;
  onRetry: (buildRunId: string) => void;
}

const TERMINAL_STATUSES = new Set<BuildRunStatus>(['succeeded', 'failed', 'cancelled']);

export function BuildRunPanel({
  buildRuns,
  selectedBuildRunId,
  cancellingBuildRunId,
  retryingBuildRunId,
  onSelect,
  onCancel,
  onRetry,
}: BuildRunPanelProps) {
  const ordered = [...buildRuns].sort((left, right) => right.requestedAt.localeCompare(left.requestedAt));

  return (
    <section className='surface build-run-panel' aria-label='Build runs'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Immutable source to OCI</p>
          <h2>Build runs</h2>
        </div>
        <span className='panel-count'>
          <Hammer size={14} /> {buildRuns.length}
        </span>
      </div>
      {ordered.length === 0 ? (
        <div className='detail-empty'>
          <Boxes size={22} />
          <strong>No build runs</strong>
          <p>Accepted source revisions and their authoritative build state will appear here.</p>
        </div>
      ) : (
        <div className='build-run-list'>
          {ordered.map((buildRun) => {
            const terminal = TERMINAL_STATUSES.has(buildRun.status);
            const cancelling = cancellingBuildRunId === buildRun.id || buildRun.status === 'cancelling';
            const retryable = buildRun.status === 'failed' || buildRun.status === 'cancelled';
            const retrying = retryingBuildRunId === buildRun.id;
            const selected = selectedBuildRunId === buildRun.id;
            return (
              <article className={`build-run-item${selected ? ' selected' : ''}`} key={buildRun.id}>
                <div className='build-run-heading'>
                  <div>
                    <strong>
                      Build {shortId(buildRun.id)} · Attempt {buildRun.attempt}
                    </strong>
                    <small>
                      source {shortId(buildRun.sourceRevisionId)} · {formatRelative(buildRun.updatedAt)}
                    </small>
                  </div>
                  <span className={`state-badge ${buildRun.status}`}>{humanize(buildRun.status)}</span>
                </div>
                <dl className='build-run-facts'>
                  <div>
                    <dt>Operation</dt>
                    <dd>{shortId(buildRun.operationId)}</dd>
                  </div>
                  <div>
                    <dt>Source digest</dt>
                    <dd>
                      {buildRun.sourceContentDigest
                        ? compactDigest(buildRun.sourceContentDigest)
                        : 'Preparing input'}
                    </dd>
                  </div>
                  <div>
                    <dt>Platform</dt>
                    <dd>{buildRun.output?.platforms.join(', ') ?? 'Pending'}</dd>
                  </div>
                  <div>
                    <dt>Artifact</dt>
                    <dd>
                      {buildRun.publishedArtifact
                        ? compactDigest(buildRun.publishedArtifact.digest)
                        : 'Not published'}
                    </dd>
                  </div>
                </dl>
                {buildRun.publishedArtifact ? (
                  <code className='build-artifact-uri'>{buildRun.publishedArtifact.uri}</code>
                ) : null}
                {buildRun.evidenceSummary ? (
                  <div className='build-run-evidence-summary'>
                    <span>
                      <ShieldCheck size={13} /> Verified evidence
                    </span>
                    <dl>
                      <div>
                        <dt>SBOM</dt>
                        <dd title={buildRun.evidenceSummary.sbomDigest}>
                          {compactDigest(buildRun.evidenceSummary.sbomDigest)}
                        </dd>
                      </div>
                      <div>
                        <dt>Provenance</dt>
                        <dd title={buildRun.evidenceSummary.provenanceDigest}>
                          {compactDigest(buildRun.evidenceSummary.provenanceDigest)}
                        </dd>
                      </div>
                      <div>
                        <dt>Signing key</dt>
                        <dd title={buildRun.evidenceSummary.signingKeyId}>
                          {compactDigest(buildRun.evidenceSummary.signingKeyId)}
                          {buildRun.evidenceSummary.signingKeyVersion === null
                            ? ''
                            : ` · v${buildRun.evidenceSummary.signingKeyVersion}`}
                        </dd>
                      </div>
                    </dl>
                  </div>
                ) : null}
                {buildRun.failure ? <output className='build-run-failure'>{buildRun.failure}</output> : null}
                <div className='build-run-actions'>
                  <button
                    className='secondary-button compact'
                    type='button'
                    aria-pressed={selected}
                    onClick={() => onSelect(buildRun.id)}
                  >
                    <SquareTerminal size={13} /> {selected ? 'Inspecting run' : 'Inspect run'}
                  </button>
                  {!terminal ? (
                    <button
                      className='danger-button compact'
                      type='button'
                      disabled={cancelling}
                      onClick={() => onCancel(buildRun.id)}
                    >
                      <Ban size={13} /> {cancelling ? 'Cancelling' : 'Cancel build'}
                    </button>
                  ) : null}
                  {retryable ? (
                    <button
                      className='secondary-button compact'
                      type='button'
                      disabled={retrying}
                      onClick={() => onRetry(buildRun.id)}
                    >
                      <RotateCcw size={13} /> {retrying ? 'Retrying' : 'Retry build'}
                    </button>
                  ) : null}
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
