import { capabilities } from '../data/product';
import { gateByCode } from '../data/roadmap';
import { CapabilityVisual } from './CapabilityVisual';
import { GateBadge } from './GateBadge';

export function CapabilityGrid() {
  return (
    <div className="cloud-capability-grid">
      {capabilities.map((capability, index) => (
        <article
          className={`cloud-capability-card is-${capability.visual}`}
          data-reveal
          key={`${capability.gate}-${capability.title}`}
          style={{ '--card-index': index } as React.CSSProperties}
        >
          <div className="cloud-capability-card-topline">
            <span>{capability.eyebrow}</span>
            <GateBadge compact gate={gateByCode(capability.gate)} />
          </div>
          <CapabilityVisual kind={capability.visual} />
          <h3>{capability.title}</h3>
          <p>{capability.body}</p>
          <footer>
            {capability.facts.map((fact) => (
              <span key={fact}>{fact}</span>
            ))}
          </footer>
        </article>
      ))}
    </div>
  );
}
