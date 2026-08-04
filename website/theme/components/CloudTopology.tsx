const nodes = [
  { id: 'intent', label: 'Desired state', meta: 'A3S ACL' },
  { id: 'cloud', label: 'Cloud', meta: 'control plane' },
  { id: 'orm', label: 'A3S ORM', meta: 'typed SQL' },
  { id: 'flow', label: 'A3S Flow', meta: 'durable work' },
  { id: 'agent', label: 'Node Agent', meta: 'outbound mTLS' },
  { id: 'box', label: 'A3S Box', meta: 'sole provider' },
  { id: 'gateway', label: 'Gateway', meta: 'live traffic' },
  { id: 'evidence', label: 'Evidence', meta: 'health / logs' },
] as const;

const paths = [
  'M88 290 C145 290 151 211 224 211',
  'M294 260 C294 332 196 346 196 423',
  'M340 214 C405 214 414 132 479 132',
  'M532 172 C532 222 574 222 574 275',
  'M574 343 C574 397 511 409 511 467',
  'M559 484 C624 484 632 400 675 400',
  'M481 486 C421 486 414 542 349 542',
  'M297 522 C233 506 245 343 279 278',
] as const;

export function CloudTopology() {
  return (
    <figure className="cloud-topology" aria-labelledby="topology-caption">
      <div className="cloud-topology-halo" aria-hidden="true" />
      <svg
        aria-hidden="true"
        className="cloud-topology-lines"
        viewBox="0 0 720 600"
      >
        <defs>
          <linearGradient id="topology-line" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#7ff0bc" stopOpacity=".18" />
            <stop offset=".5" stopColor="#a6f6d2" stopOpacity=".72" />
            <stop offset="1" stopColor="#7ff0bc" stopOpacity=".14" />
          </linearGradient>
        </defs>
        {paths.map((path, index) => (
          <g key={path}>
            <path className="cloud-topology-path" d={path} />
            <path
              className="cloud-topology-signal"
              d={path}
              style={{ animationDelay: `${index * -0.73}s` }}
            />
          </g>
        ))}
      </svg>
      <div
        className="cloud-topology-orbit cloud-topology-orbit--outer"
        aria-hidden="true"
      />
      <div
        className="cloud-topology-orbit cloud-topology-orbit--inner"
        aria-hidden="true"
      />
      {nodes.map((node) => (
        <div
          className={`cloud-topology-node is-${node.id}`}
          data-node={node.id}
          key={node.id}
        >
          <i aria-hidden="true" />
          <span>{node.label}</span>
          <small>{node.meta}</small>
        </div>
      ))}
      <figcaption id="topology-caption">
        Intent moves through durable control. Live traffic never crosses the
        Cloud control plane.
      </figcaption>
    </figure>
  );
}
