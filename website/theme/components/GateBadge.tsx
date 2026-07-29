import type { RoadmapGate } from '../data/roadmap';

type GateBadgeProps = {
  compact?: boolean;
  gate: RoadmapGate;
};

export function GateBadge({ compact = false, gate }: GateBadgeProps) {
  return (
    <span
      className={`cloud-gate-badge is-${gate.statusKind}${compact ? ' is-compact' : ''}`}
      title={`${gate.code}: ${gate.status}`}
    >
      <i aria-hidden="true" />
      <b>{gate.code}</b>
      {!compact && <span>{gate.status}</span>}
    </span>
  );
}
