import { cp, mkdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const websiteRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const documentationOutput = path.join(websiteRoot, 'docs_build');
const documentationTarget = path.join(websiteRoot, 'doc_build', 'docs');

await rm(documentationTarget, { force: true, recursive: true });
await mkdir(documentationTarget, { recursive: true });
await cp(documentationOutput, documentationTarget, { recursive: true });

console.log('Assembled the versioned documentation under doc_build/docs/.');
