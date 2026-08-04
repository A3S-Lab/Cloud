import { futureTracks } from '../data/product';
import { gateByCode } from '../data/roadmap';
import { GateBadge } from './GateBadge';

export function FutureHorizons() {
  return (
    <div
      aria-label="Product horizons"
      className="cloud-horizon-track"
      role="list"
    >
      {futureTracks.map((track, index) => (
        <article
          className={`cloud-horizon-card is-track-${index}`}
          data-reveal
          key={track.gate}
          role="listitem"
          style={{ '--track-index': index } as React.CSSProperties}
        >
          <header>
            <GateBadge compact gate={gateByCode(track.gate)} />
          </header>
          <div className="cloud-horizon-signal" aria-hidden="true">
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
