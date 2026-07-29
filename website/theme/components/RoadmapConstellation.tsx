import { useMemo, useState } from 'react';
import {
  roadmapGates,
  type RoadmapGate,
  type StatusKind,
} from '../data/roadmap';
import { GateBadge } from './GateBadge';

type RoadmapFilter = 'all' | StatusKind;

const filters: Array<{ key: RoadmapFilter; label: string }> = [
  { key: 'all', label: 'All gates' },
  { key: 'verified', label: 'Verified' },
  { key: 'in-progress', label: 'In progress' },
  { key: 'planned', label: 'Planned' },
  { key: 'historical', label: 'Re-certification' },
];

function selectGate(gates: RoadmapGate[], current: string) {
  return gates.find((gate) => gate.code === current) ?? gates[0];
}

export function RoadmapConstellation() {
  const [filter, setFilter] = useState<RoadmapFilter>('all');
  const [activeCode, setActiveCode] = useState('F0');
  const visibleGates = useMemo(
    () =>
      filter === 'all'
        ? roadmapGates
        : roadmapGates.filter((gate) => gate.statusKind === filter),
    [filter],
  );
  const activeGate = selectGate(visibleGates, activeCode);

  return (
    <div className="cloud-roadmap-shell">
      <div
        className="cloud-roadmap-toolbar"
        aria-label="Roadmap status filters"
      >
        {filters.map((item) => {
          const count =
            item.key === 'all'
              ? roadmapGates.length
              : roadmapGates.filter((gate) => gate.statusKind === item.key)
                  .length;
          return (
            <button
              aria-pressed={filter === item.key}
              className={filter === item.key ? 'is-active' : ''}
              key={item.key}
              onClick={() => {
                setFilter(item.key);
                const nextGates =
                  item.key === 'all'
                    ? roadmapGates
                    : roadmapGates.filter(
                        (gate) => gate.statusKind === item.key,
                      );
                if (!nextGates.some((gate) => gate.code === activeCode)) {
                  setActiveCode(nextGates[0]?.code ?? 'F0');
                }
              }}
              type="button"
            >
              <span>{item.label}</span>
              <b>{String(count).padStart(2, '0')}</b>
            </button>
          );
        })}
      </div>

      <div className="cloud-roadmap-map">
        <svg aria-hidden="true" viewBox="0 0 1200 420">
          <path d="M30 220C164 26 272 386 420 182S650 76 772 246s245 126 398-80" />
          <path d="M42 312c178-68 230-256 406-96s262 106 344-18 214-94 364 40" />
        </svg>
        <div className="cloud-roadmap-cards">
          {visibleGates.map((gate, index) => (
            <button
              aria-pressed={activeGate?.code === gate.code}
              className={`cloud-roadmap-card is-${gate.statusKind}${
                activeGate?.code === gate.code ? ' is-active' : ''
              }`}
              key={gate.code}
              onClick={() => setActiveCode(gate.code)}
              style={{ '--gate-index': index } as React.CSSProperties}
              type="button"
            >
              <i aria-hidden="true" />
              <b>{gate.code}</b>
              <span>{gate.name}</span>
            </button>
          ))}
        </div>
      </div>

      {activeGate && (
        <section className="cloud-roadmap-detail" aria-live="polite">
          <div>
            <GateBadge gate={activeGate} />
            <span>ROADMAP GATE</span>
          </div>
          <h3>{activeGate.name}</h3>
          <p>{activeGate.outcome}</p>
          <a href="https://github.com/A3S-Lab/Cloud/blob/main/ROADMAP.md">
            Read evidence and dependencies <span aria-hidden="true">↗</span>
          </a>
        </section>
      )}
    </div>
  );
}
