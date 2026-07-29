import type { CapabilityVisual as VisualKind } from '../data/product';

type CapabilityVisualProps = {
  kind: VisualKind;
};

function IntentVisual() {
  return (
    <div className="cloud-mini-ledger">
      {['desired_state', 'idempotency', 'outbox_event'].map((label, index) => (
        <span key={label} style={{ animationDelay: `${index * 0.35}s` }}>
          <i />
          <code>{label}</code>
          <b>committed</b>
        </span>
      ))}
    </div>
  );
}

function BoxVisual() {
  return (
    <div className="cloud-mini-box">
      <span className="cloud-mini-box-top">Runtime contract</span>
      <span className="cloud-mini-box-core">
        <i />
        A3S Box
      </span>
      <span className="cloud-mini-box-unit">immutable unit</span>
    </div>
  );
}

function DeliveryVisual() {
  return (
    <div className="cloud-mini-delivery">
      {['commit', 'OCI', 'signed'].map((label) => (
        <span key={label}>
          <i />
          {label}
        </span>
      ))}
      <b aria-hidden="true" />
    </div>
  );
}

function GatewayVisual() {
  return (
    <div className="cloud-mini-gateway">
      <span className="cloud-mini-gateway-entry">TLS</span>
      <i className="cloud-mini-gateway-route" />
      <span className="cloud-mini-gateway-core">Gateway</span>
      <div>
        <span>target 01</span>
        <span>target 02</span>
      </div>
    </div>
  );
}

function RecoveryVisual() {
  return (
    <div className="cloud-mini-recovery">
      <div>
        {[1, 2, 3, 4, 5].map((value) => (
          <i key={value} />
        ))}
      </div>
      <span>
        <b /> receipt settled
      </span>
      <code>04 → replay → 05</code>
    </div>
  );
}

function SurfacesVisual() {
  return (
    <div className="cloud-mini-surfaces">
      <strong>Cloud</strong>
      {['REST', 'CLI', 'WEB', 'MCP'].map((surface, index) => (
        <span className={`is-surface-${index}`} key={surface}>
          {surface}
        </span>
      ))}
    </div>
  );
}

export function CapabilityVisual({ kind }: CapabilityVisualProps) {
  return (
    <div aria-hidden="true" className={`cloud-capability-visual is-${kind}`}>
      {kind === 'intent' && <IntentVisual />}
      {kind === 'box' && <BoxVisual />}
      {kind === 'delivery' && <DeliveryVisual />}
      {kind === 'gateway' && <GatewayVisual />}
      {kind === 'recovery' && <RecoveryVisual />}
      {kind === 'surfaces' && <SurfacesVisual />}
    </div>
  );
}
