import { ArrowDownLeft, ArrowUpRight, Box, Crosshair, ExternalLink, X } from 'lucide-react';
import type { CSSProperties } from 'react';
import { ARCHITECTURE_GRAPH, ARCHITECTURE_STATUS_META, type ArchitectureNode } from '../architecture';
import { ARCHITECTURE_CARRIERS, ARCHITECTURE_HOSTING_RELATIONSHIPS } from '../topology';

interface NodeInspectorProps {
  node?: ArchitectureNode;
  onClose: () => void;
  onFocus: () => void;
  onSelectNode: (nodeId: string) => void;
}

export function NodeInspector({ node, onClose, onFocus, onSelectNode }: NodeInspectorProps) {
  if (!node) return null;

  const status = ARCHITECTURE_STATUS_META[node.status];
  const inbound = ARCHITECTURE_GRAPH.edges.filter((edge) => edge.to === node.id);
  const outbound = ARCHITECTURE_GRAPH.edges.filter((edge) => edge.from === node.id);
  const carriers = ARCHITECTURE_CARRIERS.filter((carrier) => carrier.memberNodeIds.includes(node.id));
  const hostingRelationships = ARCHITECTURE_HOSTING_RELATIONSHIPS.filter(
    (relationship) =>
      relationship.hostNodeIds.includes(node.id) || relationship.guestNodeIds.includes(node.id)
  );

  return (
    <aside
      className='node-inspector'
      aria-labelledby='node-inspector-title'
      style={{ '--node-status': status.color } as CSSProperties}
    >
      <div className='inspector-drag-handle' aria-hidden='true' />
      <div className='inspector-heading'>
        <div>
          <span className='inspector-eyebrow'>{node.eyebrow}</span>
          <h2 id='node-inspector-title'>{node.label}</h2>
        </div>
        <button
          type='button'
          className='icon-button inspector-close'
          onClick={onClose}
          aria-label='Close component details'
        >
          <X size={17} aria-hidden='true' />
        </button>
      </div>

      <div className='inspector-badges'>
        <span className='status-badge'>
          <i aria-hidden='true' />
          {status.label}
        </span>
        <span className='gate-badge'>{node.gate}</span>
      </div>

      <p className='inspector-summary'>{node.summary}</p>

      <section className='inspector-section'>
        <h3>Owns</h3>
        <ul className='ownership-list'>
          {node.owns.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      </section>

      <section className='inspector-section boundary-section'>
        <h3>Boundary rule</h3>
        <p>{node.boundary}</p>
      </section>

      {carriers.length > 0 || hostingRelationships.length > 0 ? (
        <section className='inspector-section'>
          <h3>Runtime placement</h3>
          <div className='placement-list'>
            {carriers.map((carrier) => (
              <div className='carrier-membership' key={carrier.id}>
                <Box size={13} aria-hidden='true' />
                <span>
                  <small>Mounted on carrier chassis</small>
                  <strong>{carrier.label}</strong>
                </span>
              </div>
            ))}
            {hostingRelationships.flatMap((relationship) => {
              const isHost = relationship.hostNodeIds.includes(node.id);
              const relatedNodeIds = isHost ? relationship.guestNodeIds : relationship.hostNodeIds;
              return relatedNodeIds.map((relatedNodeId) => (
                <HostingButton
                  key={`${relationship.id}:${relatedNodeId}`}
                  label={relationship.label}
                  nodeId={relatedNodeId}
                  relation={isHost ? relationship.hostAction : relationship.guestAction}
                  onSelectNode={onSelectNode}
                />
              ));
            })}
          </div>
        </section>
      ) : null}

      {inbound.length > 0 || outbound.length > 0 ? (
        <section className='inspector-section'>
          <h3>Connected signals</h3>
          <div className='connection-list'>
            {inbound.map((edge) => (
              <ConnectionButton
                key={edge.id}
                direction='inbound'
                label={edge.label}
                nodeId={edge.from}
                onSelectNode={onSelectNode}
              />
            ))}
            {outbound.map((edge) => (
              <ConnectionButton
                key={edge.id}
                direction='outbound'
                label={edge.label}
                nodeId={edge.to}
                onSelectNode={onSelectNode}
              />
            ))}
          </div>
        </section>
      ) : null}

      <div className='inspector-actions'>
        <button type='button' className='primary-button' onClick={onFocus}>
          <Crosshair size={15} aria-hidden='true' />
          Focus in scene
        </button>
        <a className='secondary-button' href={node.docsUrl} target='_blank' rel='noreferrer'>
          Read design
          <ExternalLink size={14} aria-hidden='true' />
        </a>
      </div>
    </aside>
  );
}

function HostingButton({
  label,
  nodeId,
  relation,
  onSelectNode,
}: {
  label: string;
  nodeId: string;
  relation: string;
  onSelectNode: (nodeId: string) => void;
}) {
  const relatedNode = ARCHITECTURE_GRAPH.nodes.find((candidate) => candidate.id === nodeId);
  if (!relatedNode) return null;
  return (
    <button type='button' className='hosting-relationship' onClick={() => onSelectNode(nodeId)}>
      <Box size={13} aria-hidden='true' />
      <span>
        <small>
          {relation} · {label}
        </small>
        <strong>{relatedNode.label}</strong>
      </span>
    </button>
  );
}

function ConnectionButton({
  direction,
  label,
  nodeId,
  onSelectNode,
}: {
  direction: 'inbound' | 'outbound';
  label: string;
  nodeId: string;
  onSelectNode: (nodeId: string) => void;
}) {
  const otherNode = ARCHITECTURE_GRAPH.nodes.find((candidate) => candidate.id === nodeId);
  if (!otherNode) return null;

  return (
    <button type='button' onClick={() => onSelectNode(nodeId)}>
      {direction === 'inbound' ? (
        <ArrowDownLeft size={13} aria-hidden='true' />
      ) : (
        <ArrowUpRight size={13} aria-hidden='true' />
      )}
      <span>
        <small>{label}</small>
        <strong>{otherNode.label}</strong>
      </span>
    </button>
  );
}
