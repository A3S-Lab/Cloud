import { Globe2, ShieldCheck } from 'lucide-react';
import type { GatewayCertificate, Route, Workload } from '../../types/api';
import { compactDigest, formatTimestamp, humanize, shortId } from './console-format';

interface EdgeStatusPanelProps {
  workload: Workload | undefined;
  routes: Route[];
  certificates: GatewayCertificate[];
}

export function EdgeStatusPanel({ workload, routes, certificates }: EdgeStatusPanelProps) {
  const certificateById = new Map(certificates.map((certificate) => [certificate.id, certificate]));

  return (
    <section className='surface edge-status-panel' aria-label='Route and certificate state'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Authoritative edge projection</p>
          <h2>Routes and certificates</h2>
        </div>
        <span className='panel-count'>
          <Globe2 size={14} /> {routes.length}
        </span>
      </div>
      {!workload || routes.length === 0 ? (
        <div className='detail-empty'>
          <Globe2 size={21} />
          <strong>No route projection</strong>
          <p>Reachability appears only after Cloud owns a route for this workload.</p>
        </div>
      ) : (
        <div className='edge-route-list'>
          {routes.map((route) => {
            const certificate = route.gatewayCertificateId
              ? certificateById.get(route.gatewayCertificateId)
              : undefined;
            const generation = workload.deployments.find(
              (deployment) => deployment.revision.id === route.workloadRevisionId
            )?.revision.generation;
            return (
              <article className='edge-route' key={route.id}>
                <div className='edge-route-heading'>
                  <div>
                    <strong>
                      {route.gatewayCertificateId ? 'https' : 'http'}://{route.hostname}
                      {route.pathPrefix}
                    </strong>
                    <small>
                      {generation ? `Generation ${generation}` : shortId(route.workloadRevisionId)}
                    </small>
                  </div>
                  <span className={`state-badge ${route.state}`}>{humanize(route.state)}</span>
                </div>
                <dl className='edge-facts'>
                  <div>
                    <dt>Gateway node</dt>
                    <dd>{shortId(route.gatewayNodeId)}</dd>
                  </div>
                  <div>
                    <dt>Gateway revision</dt>
                    <dd>{route.gatewayRevision ?? 'Not acknowledged'}</dd>
                  </div>
                  <div>
                    <dt>Activated</dt>
                    <dd>{formatTimestamp(route.activatedAt)}</dd>
                  </div>
                  <div>
                    <dt>Snapshot</dt>
                    <dd>{route.snapshotDigest ? compactDigest(route.snapshotDigest) : 'Not published'}</dd>
                  </div>
                </dl>
                {route.failure ? <output className='edge-failure'>{route.failure}</output> : null}
                <CertificateProjection certificateId={route.gatewayCertificateId} certificate={certificate} />
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}

function CertificateProjection({
  certificateId,
  certificate,
}: {
  certificateId: string | null;
  certificate: GatewayCertificate | undefined;
}) {
  if (!certificateId) {
    return (
      <div className='certificate-projection unbound'>
        <ShieldCheck size={16} />
        <span>
          <strong>No managed certificate bound</strong>
          <small>This route projection does not reference a Gateway certificate.</small>
        </span>
      </div>
    );
  }
  if (!certificate) {
    return (
      <div className='certificate-projection missing'>
        <ShieldCheck size={16} />
        <span>
          <strong>Certificate projection unavailable</strong>
          <small>Referenced certificate {shortId(certificateId)} is absent from this snapshot.</small>
        </span>
      </div>
    );
  }
  return (
    <div className='certificate-projection'>
      <ShieldCheck size={16} />
      <span>
        <span className='certificate-title'>
          <strong>{certificate.dnsNames.join(', ')}</strong>
          <em className={`state-badge ${certificate.state}`}>{humanize(certificate.state)}</em>
        </span>
        <small>
          Fingerprint {certificate.fingerprint ? compactDigest(certificate.fingerprint) : 'not issued'} ·
          expires {formatTimestamp(certificate.expiresAt)}
        </small>
        {certificate.failure ? <small className='certificate-failure'>{certificate.failure}</small> : null}
      </span>
    </div>
  );
}
