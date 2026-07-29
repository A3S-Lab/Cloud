import rawRoadmap from '../generated/roadmap.json';

export type StatusKind = 'verified' | 'in-progress' | 'planned' | 'historical';

export type RoadmapGate = {
  code: string;
  name: string;
  outcome: string;
  status: string;
  statusKind: StatusKind;
};

type RoadmapData = {
  source: string;
  gates: RoadmapGate[];
};

export const roadmap = rawRoadmap as RoadmapData;
export const roadmapGates = roadmap.gates;

export function gateByCode(code: string) {
  const gate = roadmapGates.find((candidate) => candidate.code === code);
  if (!gate) throw new Error(`Unknown roadmap gate: ${code}`);
  return gate;
}
