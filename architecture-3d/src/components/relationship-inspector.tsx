import { ArrowRight, Box, GitBranch, X } from 'lucide-react';
import type { CSSProperties } from 'react';
import { ARCHITECTURE_GRAPH } from '../architecture';
import type { ArchitectureRelationshipSelection } from '../selection';
import { ARCHITECTURE_HOSTING_RELATIONSHIPS } from '../topology';

interface RelationshipInspectorProps {
  selection?: ArchitectureRelationshipSelection;
  onClose: () => void;
  onSelectNode: (nodeId: string) => void;
}

export function RelationshipInspector({ selection, onClose, onSelectNode }: RelationshipInspectorProps) {
  if (!selection) return null;

  const businessEdge =
    selection.kind === 'business-edge'
      ? ARCHITECTURE_GRAPH.edges.find((edge) => edge.id === selection.id)
      : undefined;
  const structuralEdge =
    selection.kind === 'structural-edge'
      ? ARCHITECTURE_HOSTING_RELATIONSHIPS.find((relationship) => relationship.id === selection.id)
      : undefined;
  if (!businessEdge && !structuralEdge) return null;

  const color = businessEdge
    ? (ARCHITECTURE_GRAPH.journeys.find((journey) =>
        businessEdge.journeys.includes(journey.id as (typeof businessEdge.journeys)[number])
      )?.color ?? '#72b7ff')
    : (structuralEdge?.color ?? '#71d5c3');
  const sourceNodeIds = businessEdge ? [businessEdge.from] : (structuralEdge?.hostNodeIds ?? []);
  const targetNodeIds = businessEdge ? [businessEdge.to] : (structuralEdge?.guestNodeIds ?? []);
  const summary = businessEdge?.summary ?? structuralEdge?.description ?? '';
  const boundary = businessEdge?.boundary ?? structuralEdge?.boundary ?? '';
  const label = businessEdge?.label ?? structuralEdge?.label ?? '';

  return (
    <aside
      className='node-inspector relationship-inspector'
      aria-labelledby='relationship-inspector-title'
      style={{ '--node-status': color } as CSSProperties}
    >
      <div className='inspector-drag-handle' aria-hidden='true' />
      <div className='inspector-heading'>
        <div>
          <span className='inspector-eyebrow'>{businessEdge ? 'Business flow' : 'Structure / hosting'}</span>
          <h2 id='relationship-inspector-title'>{label}</h2>
        </div>
        <button
          type='button'
          className='icon-button inspector-close'
          onClick={onClose}
          aria-label='Close relationship details'
        >
          <X size={17} aria-hidden='true' />
        </button>
      </div>

      <div className='inspector-badges'>
        <span className='status-badge'>
          {businessEdge ? <GitBranch size={12} aria-hidden='true' /> : <Box size={12} aria-hidden='true' />}
          {businessEdge ? 'Directed signal' : 'Structural contract'}
        </span>
        <span className='gate-badge'>{selection.id}</span>
      </div>

      <p className='inspector-summary'>{summary}</p>

      <section className='inspector-section'>
        <h3>{businessEdge ? 'Signal path' : 'Placement / cooperation path'}</h3>
        <div className='relationship-route'>
          <EndpointGroup
            eyebrow={businessEdge ? 'From' : (structuralEdge?.hostAction ?? 'Host')}
            nodeIds={sourceNodeIds}
            onSelectNode={onSelectNode}
          />
          <span className='relationship-direction' aria-hidden='true'>
            <ArrowRight size={17} />
          </span>
          <EndpointGroup
            eyebrow={businessEdge ? 'To' : (structuralEdge?.guestAction ?? 'Guest')}
            nodeIds={targetNodeIds}
            onSelectNode={onSelectNode}
          />
        </div>
        <p className='relationship-navigation-hint'>
          Select either endpoint to open its component HUD and focus it in the scene.
        </p>
      </section>

      {businessEdge ? (
        <>
          <section className='inspector-section'>
            <h3>What crosses this boundary</h3>
            <ul className='ownership-list'>
              {businessEdge.transfers.map((transfer) => (
                <li key={transfer}>{transfer}</li>
              ))}
            </ul>
          </section>

          <section className='inspector-section'>
            <h3>Used by simulations</h3>
            <div className='relationship-journeys'>
              {businessEdge.journeys.map((journeyId) => {
                const journey = ARCHITECTURE_GRAPH.journeys.find((candidate) => candidate.id === journeyId);
                return journey ? (
                  <span key={journey.id} style={{ '--journey-color': journey.color } as CSSProperties}>
                    <i aria-hidden='true' />
                    {journey.label}
                  </span>
                ) : null;
              })}
            </div>
          </section>
        </>
      ) : (
        <section className='inspector-section'>
          <h3>Directional semantics</h3>
          <dl className='relationship-semantics'>
            <div>
              <dt>Host / coordinator</dt>
              <dd>{structuralEdge?.hostAction}</dd>
            </div>
            <div>
              <dt>Guest / collaborator</dt>
              <dd>{structuralEdge?.guestAction}</dd>
            </div>
          </dl>
        </section>
      )}

      <section className='inspector-section boundary-section'>
        <h3>Boundary rule</h3>
        <p>{boundary}</p>
      </section>
    </aside>
  );
}

function EndpointGroup({
  eyebrow,
  nodeIds,
  onSelectNode,
}: {
  eyebrow: string;
  nodeIds: readonly string[];
  onSelectNode: (nodeId: string) => void;
}) {
  return (
    <div className='relationship-endpoints'>
      {nodeIds.map((nodeId) => {
        const node = ARCHITECTURE_GRAPH.nodes.find((candidate) => candidate.id === nodeId);
        if (!node) return null;
        return (
          <button type='button' key={node.id} onClick={() => onSelectNode(node.id)}>
            <small>{eyebrow}</small>
            <strong>{node.label}</strong>
          </button>
        );
      })}
    </div>
  );
}
