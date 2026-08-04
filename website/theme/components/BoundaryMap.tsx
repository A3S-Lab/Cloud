import { gateByCode } from '../data/roadmap';
import { GateBadge } from './GateBadge';

const boundaries = [
  {
    id: 'cloud',
    gate: 'F0',
    name: 'A3S Cloud',
    role: 'Desired-state authority',
    owns: 'Tenancy / policy / placement / rollout / operations',
  },
  {
    id: 'box',
    gate: 'BX0',
    name: 'A3S Box',
    role: 'Sole execution provider',
    owns: 'Isolation / lifecycle / resources / local evidence',
  },
  {
    id: 'gateway',
    gate: 'H0',
    name: 'A3S Gateway',
    role: 'Traffic data plane',
    owns: 'TLS / streaming / enforcement / healthy dispatch',
  },
  {
    id: 'power',
    gate: 'PW0',
    name: 'A3S Power',
    role: 'Typed inference backend',
    owns: 'Power Service / MicroVM / TEE execution evidence',
  },
] as const;

export function BoundaryMap() {
  return (
    <div className="cloud-boundary-map">
      <svg aria-hidden="true" viewBox="0 0 1000 550">
        <path className="is-control" d="M296 262C388 142 442 133 503 116" />
        <path className="is-command" d="M296 288C380 374 423 409 503 420" />
        <path className="is-traffic" d="M558 136C716 147 720 259 789 264" />
        <path className="is-backend" d="M556 402C706 390 716 297 789 292" />
      </svg>
      {boundaries.map((boundary) => (
        <article
          className={`cloud-boundary-node is-${boundary.id}`}
          data-reveal
          key={boundary.id}
        >
          <header>
            <GateBadge compact gate={gateByCode(boundary.gate)} />
          </header>
          <h3>{boundary.name}</h3>
          <p className="cloud-boundary-role">{boundary.role}</p>
          <p>{boundary.owns}</p>
        </article>
      ))}
      <div className="cloud-boundary-rule">
        <span>CONTROL</span>
        <strong>Cloud stays off the live request path.</strong>
        <span>TRAFFIC</span>
      </div>
    </div>
  );
}
