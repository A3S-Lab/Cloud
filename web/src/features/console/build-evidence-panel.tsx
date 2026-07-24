import { Download, FileJson2, ShieldCheck } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import type { BuildEvidence, BuildRun } from '../../types/api';
import { formatTimestamp } from './console-format';

interface BuildEvidencePanelProps {
  api: CloudApi;
  organizationId: string | null;
  buildRun: BuildRun | null;
}

const MAX_PREVIEW_CHARACTERS = 200_000;

export function BuildEvidencePanel({ api, organizationId, buildRun }: BuildEvidencePanelProps) {
  const [evidence, setEvidence] = useState<BuildEvidence | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef<AbortController | null>(null);
  const activeSelection = useRef('');
  const summary = buildRun?.evidenceSummary ?? null;
  const selectionKey = organizationId && buildRun ? `${organizationId}:${buildRun.id}` : '';

  useEffect(() => {
    activeSelection.current = selectionKey;
    request.current?.abort();
    request.current = null;
    setEvidence(null);
    setLoading(false);
    setError(null);
    return () => request.current?.abort();
  }, [selectionKey]);

  const preview = useMemo(() => {
    if (!evidence) return null;
    const document = JSON.stringify(evidence, null, 2);
    return {
      text:
        document.length > MAX_PREVIEW_CHARACTERS
          ? `${document.slice(0, MAX_PREVIEW_CHARACTERS)}\n… preview truncated; download the complete document.`
          : document,
      truncated: document.length > MAX_PREVIEW_CHARACTERS,
    };
  }, [evidence]);

  const loadEvidence = async (): Promise<BuildEvidence | null> => {
    if (evidence) return evidence;
    if (!organizationId || !buildRun || !summary || loading) return null;
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    const requestedSelection = selectionKey;
    setLoading(true);
    setError(null);
    try {
      const loaded = await api.getBuildEvidence(organizationId, buildRun.id, controller.signal);
      if (controller.signal.aborted || activeSelection.current !== requestedSelection) return null;
      if (
        loaded.buildRunId !== buildRun.id ||
        loaded.verificationState !== summary.verificationState ||
        loaded.sbomDigest !== summary.sbomDigest ||
        loaded.provenanceDigest !== summary.provenanceDigest ||
        loaded.signingKey.keyId !== summary.signingKeyId
      ) {
        throw new Error('Build evidence response did not match the selected BuildRun.');
      }
      setEvidence(loaded);
      return loaded;
    } catch (cause) {
      if (!controller.signal.aborted) {
        setError(cause instanceof Error ? cause.message : 'Build evidence could not be loaded.');
      }
      return null;
    } finally {
      if (request.current === controller) {
        request.current = null;
        setLoading(false);
      }
    }
  };

  const download = async () => {
    const document = evidence ?? (await loadEvidence());
    if (document) downloadEvidence(document);
  };

  return (
    <section className='surface build-evidence-panel' aria-label='Build evidence'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Supply-chain integrity</p>
          <h2>Build evidence</h2>
        </div>
        {summary ? (
          <span className='evidence-verified'>
            <ShieldCheck size={14} /> Verified
          </span>
        ) : null}
      </div>

      {!buildRun ? (
        <div className='detail-empty'>
          <FileJson2 size={22} />
          <strong>Select a build run</strong>
          <p>Choose a BuildRun to inspect its signed SBOM and provenance state.</p>
        </div>
      ) : !summary ? (
        <div className='detail-empty'>
          <FileJson2 size={22} />
          <strong>
            {buildRun.status === 'attesting' ? 'Attestation in progress' : 'No evidence available'}
          </strong>
          <p>
            {buildRun.status === 'attesting'
              ? 'Cloud is generating and signing the immutable evidence document.'
              : 'This BuildRun has not produced verified supply-chain evidence.'}
          </p>
        </div>
      ) : (
        <div className='build-evidence-content'>
          <div className='build-evidence-status'>
            <ShieldCheck size={20} />
            <div>
              <strong>Verified evidence</strong>
              <span>
                {summary.signingKeyAlgorithm}
                {summary.signingKeyVersion === null ? '' : ` · key version ${summary.signingKeyVersion}`}
                {' · '}
                {formatTimestamp(summary.attestedAt)}
              </span>
            </div>
          </div>
          <dl className='build-evidence-facts'>
            <div>
              <dt>SBOM digest</dt>
              <dd>{summary.sbomDigest}</dd>
            </div>
            <div>
              <dt>Provenance digest</dt>
              <dd>{summary.provenanceDigest}</dd>
            </div>
            <div>
              <dt>Signing key ID</dt>
              <dd>{summary.signingKeyId}</dd>
            </div>
            <div>
              <dt>Evidence schema</dt>
              <dd>{summary.schema}</dd>
            </div>
          </dl>
          <div className='build-evidence-actions'>
            <button
              className='secondary-button compact'
              type='button'
              disabled={loading}
              onClick={() => void loadEvidence()}
            >
              <FileJson2 size={13} /> {loading ? 'Loading evidence' : 'View evidence JSON'}
            </button>
            <button
              className='secondary-button compact'
              type='button'
              disabled={loading}
              onClick={() => void download()}
            >
              <Download size={13} /> {loading ? 'Loading evidence' : 'Download JSON'}
            </button>
          </div>
          {error ? (
            <output className='build-evidence-error' role='alert'>
              {error}
            </output>
          ) : null}
          {preview ? (
            <div className='build-evidence-document'>
              <div>
                <strong>Evidence JSON</strong>
                <span>{preview.truncated ? 'Bounded preview' : 'Complete document'}</span>
              </div>
              <pre>{preview.text}</pre>
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}

function downloadEvidence(evidence: BuildEvidence) {
  const blob = new Blob([`${JSON.stringify(evidence, null, 2)}\n`], {
    type: 'application/json',
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `a3s-build-${safeFileSegment(evidence.buildRunId)}-evidence.json`;
  link.hidden = true;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

function safeFileSegment(value: string): string {
  return value.replaceAll(/[^a-zA-Z0-9_-]/g, '-').slice(0, 128);
}
