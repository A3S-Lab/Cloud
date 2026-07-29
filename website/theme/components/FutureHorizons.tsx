import { futureTracks } from '../data/product';
import { gateByCode } from '../data/roadmap';
import { GateBadge } from './GateBadge';

export function FutureHorizons() {
  return (
    <div className="cloud-future-grid">
      {futureTracks.map((track, index) => (
        <article
          className={`cloud-future-card is-track-${index}`}
          data-reveal
          key={track.gate}
          style={{ '--track-index': index } as React.CSSProperties}
        >
          <header>
            <span>{track.label}</span>
            <GateBadge compact gate={gateByCode(track.gate)} />
          </header>
          <div className="cloud-future-signal" aria-hidden="true">
            <i />
            <i />
            <i />
            <i />
            <b />
          </div>
          <h3>{track.title}</h3>
          <p>{track.body}</p>
          <code>{track.signal}</code>
        </article>
      ))}
    </div>
  );
}
