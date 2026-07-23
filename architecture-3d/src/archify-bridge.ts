import { ARCHITECTURE_GRAPH } from './architecture';
import type { ArchitectureRelationshipSelection } from './selection';
import { ARCHITECTURE_HOSTING_RELATIONSHIPS } from './topology';

export const ARCHIFY_ARTIFACT_PATH = 'archify/a3s-cloud.architecture.html';

export function archifyStructuralConnectionId(
  relationshipId: string,
  hostNodeId: string,
  guestNodeId: string
): string {
  return `struct-${relationshipId}-${hostNodeId}-${guestNodeId}`;
}

export function archifyRelationSelection(relationId: string): ArchitectureRelationshipSelection | undefined {
  if (ARCHITECTURE_GRAPH.edges.some((edge) => edge.id === relationId)) {
    return { kind: 'business-edge', id: relationId };
  }
  const relationship = ARCHITECTURE_HOSTING_RELATIONSHIPS.find((candidate) =>
    relationId.startsWith(`struct-${candidate.id}-`)
  );
  return relationship ? { kind: 'structural-edge', id: relationship.id } : undefined;
}

export function primaryArchifyRelationId(selection: ArchitectureRelationshipSelection): string | undefined {
  if (selection.kind === 'business-edge') return selection.id;
  const relationship = ARCHITECTURE_HOSTING_RELATIONSHIPS.find((candidate) => candidate.id === selection.id);
  const hostNodeId = relationship?.hostNodeIds[0];
  const guestNodeId = relationship?.guestNodeIds[0];
  return relationship && hostNodeId && guestNodeId
    ? archifyStructuralConnectionId(relationship.id, hostNodeId, guestNodeId)
    : undefined;
}
