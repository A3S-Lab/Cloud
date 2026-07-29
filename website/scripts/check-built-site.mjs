import { access, readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const websiteRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const outputRoot = path.join(websiteRoot, 'doc_build');
const configuredBase = process.env.DOCS_BASE ?? '/Cloud/';
const base = `/${configuredBase.replace(/^\/+|\/+$/g, '')}/`;
const expectArchitecture = process.env.A3S_EXPECT_ARCHITECTURE === '1';
const requiredFiles = [
  '.nojekyll',
  'index.html',
  'docs/index.html',
  'a3s-cloud-mark.svg',
  'favicon.svg',
  'social-card.svg',
  ...(expectArchitecture
    ? [
        'architecture/index.html',
        'architecture/archify/a3s-cloud.architecture.html',
      ]
    : []),
];

async function collectFiles(directory, predicate) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const absolutePath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectFiles(absolutePath, predicate)));
    } else if (predicate(entry.name)) {
      files.push(absolutePath);
    }
  }
  return files;
}

async function isFile(candidate) {
  try {
    return (await stat(candidate)).isFile();
  } catch {
    return false;
  }
}

async function resolvesToBuiltFile(reference, htmlFile) {
  const cleanReference = decodeURIComponent(reference.split(/[?#]/, 1)[0]);
  let relativeReference;
  if (cleanReference.startsWith(base)) {
    relativeReference = cleanReference.slice(base.length);
  } else if (cleanReference.startsWith('/')) {
    return false;
  } else {
    relativeReference = path.relative(
      outputRoot,
      path.resolve(path.dirname(htmlFile), cleanReference),
    );
  }

  const candidates =
    relativeReference === '' || relativeReference.endsWith('/')
      ? [path.join(relativeReference, 'index.html')]
      : [
          relativeReference,
          `${relativeReference}.html`,
          path.join(relativeReference, 'index.html'),
        ];
  for (const candidate of candidates) {
    const outputPath = path.resolve(outputRoot, candidate);
    if (
      outputPath !== outputRoot &&
      !outputPath.startsWith(`${outputRoot}${path.sep}`)
    ) {
      continue;
    }
    if (await isFile(outputPath)) return true;
  }
  return false;
}

for (const file of requiredFiles) await access(path.join(outputRoot, file));

const brokenReferences = [];
const htmlFiles = await collectFiles(outputRoot, (name) =>
  name.endsWith('.html'),
);
const referencePattern = /(?:href|src)="([^"]+)"/g;

for (const htmlFile of htmlFiles) {
  const html = await readFile(htmlFile, 'utf8');
  for (const [, reference] of html.matchAll(referencePattern)) {
    if (
      reference.startsWith('#') ||
      reference.startsWith('data:') ||
      reference.startsWith('mailto:') ||
      reference.startsWith('javascript:') ||
      /^[a-z]+:\/\//i.test(reference)
    ) {
      continue;
    }
    if (!expectArchitecture && reference.startsWith(`${base}architecture`)) {
      continue;
    }
    if (!(await resolvesToBuiltFile(reference, htmlFile))) {
      brokenReferences.push(
        `${path.relative(outputRoot, htmlFile)} -> ${reference}`,
      );
    }
  }
}

if (expectArchitecture) {
  const javascriptFiles = await collectFiles(outputRoot, (name) =>
    name.endsWith('.js'),
  );
  const javascript = (
    await Promise.all(javascriptFiles.map((file) => readFile(file, 'utf8')))
  ).join('\n');
  const expectedArchifyPath = `${base}architecture/archify/a3s-cloud.architecture.html`;
  if (!javascript.includes(expectedArchifyPath)) {
    brokenReferences.push(`architecture JavaScript -> ${expectedArchifyPath}`);
  }

  const architectureText = [
    javascript,
    await readFile(
      path.join(outputRoot, 'architecture/archify/a3s-cloud.architecture.html'),
      'utf8',
    ),
  ].join('\n');
  const retiredArchitectureClaims = [
    'docker-buildkit',
    'Docker · BuildKit',
    'R0–E0 verified',
  ];
  for (const claim of retiredArchitectureClaims) {
    if (architectureText.includes(claim)) {
      brokenReferences.push(`architecture contains retired claim: ${claim}`);
    }
  }
}

if (brokenReferences.length > 0) {
  throw new Error(
    `Built-site reference check failed:\n${brokenReferences
      .map((reference) => `  - ${reference}`)
      .join('\n')}`,
  );
}

console.log(
  `Verified ${requiredFiles.length} required files and ${htmlFiles.length} HTML pages under ${base}.`,
);
