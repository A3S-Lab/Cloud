import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const websiteRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const roadmapPath = path.resolve(websiteRoot, '..', 'ROADMAP.md');
const outputPath = path.join(websiteRoot, 'theme', 'generated', 'roadmap.json');
const expectedGates = [
  'BX0',
  'PW0',
  'R0',
  'F0',
  'N0',
  'D0',
  'E0',
  'G0',
  'P0',
  'C0',
  'A0',
  'U0',
  'MCP0',
  'A1',
  'W0',
  'S0',
  'H0',
  'I0',
  'EV0',
  'AR0',
];

function normalizeStatus(status) {
  if (status.startsWith('Verified')) return 'verified';
  if (status.startsWith('In progress')) return 'in-progress';
  if (status.startsWith('Planned')) return 'planned';
  if (status.startsWith('Historical')) return 'historical';
  throw new Error(`Unsupported roadmap status: ${status}`);
}

const roadmap = await readFile(roadmapPath, 'utf8');
const rowPattern =
  /^\| `([A-Z][A-Z0-9]*)` — ([^|]+?) \| ([^|]+?) \| ([^|]+?) \|$/gm;
const gates = [...roadmap.matchAll(rowPattern)].map(
  ([, code, name, outcome, status]) => ({
    code,
    name: name.trim(),
    outcome: outcome.trim(),
    status: status.trim(),
    statusKind: normalizeStatus(status.trim()),
  }),
);

if (gates.map(({ code }) => code).join(',') !== expectedGates.join(',')) {
  throw new Error(
    `Roadmap gate matrix changed. Expected ${expectedGates.join(', ')}, received ${gates
      .map(({ code }) => code)
      .join(', ')}`,
  );
}

const generated = `${JSON.stringify(
  {
    source: '../ROADMAP.md#3-current-roadmap',
    gates,
  },
  null,
  2,
)}\n`;

if (process.argv.includes('--check')) {
  const current = await readFile(outputPath, 'utf8');
  if (current !== generated) {
    throw new Error(
      'Generated roadmap data is stale. Run `npm run generate:roadmap`.',
    );
  }
  console.log(`Verified ${gates.length} roadmap gates against ROADMAP.md.`);
} else {
  await writeFile(outputPath, generated, 'utf8');
  console.log(`Generated ${gates.length} roadmap gates from ROADMAP.md.`);
}
