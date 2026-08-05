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
const documentationRoot = path.join(websiteRoot, 'documentation');
const documentationRegistry = JSON.parse(
  await readFile(path.join(documentationRoot, 'versions.json'), 'utf8'),
);
const documentationLanguages = documentationRegistry.languages.map(
  (language) => language.id,
);
const documentationRequiredFiles = [];
for (const version of documentationRegistry.versions) {
  for (const language of documentationLanguages) {
    const languageRoot = path.join(documentationRoot, version, language);
    const sourceFiles = await collectFiles(languageRoot, (name) =>
      /\.mdx?$/.test(name),
    );
    const prefix = documentationPrefix(version, language);
    documentationRequiredFiles.push(
      ...sourceFiles.map((file) =>
        path.posix.join(
          ...prefix,
          path
            .relative(languageRoot, file)
            .split(path.sep)
            .join('/')
            .replace(/\.mdx?$/, '.html'),
        ),
      ),
    );
  }
}
const requiredFiles = [
  '.nojekyll',
  'index.html',
  ...documentationRequiredFiles,
  'a3s-os-logo.png',
  'a3s-edge-cloud-foundation-color.png',
  'social-card.svg',
  ...(expectArchitecture
    ? [
        'architecture/index.html',
        'architecture/archify/a3s-cloud.architecture.html',
      ]
    : []),
];

function documentationPrefix(version, language) {
  return [
    'docs',
    version === documentationRegistry.default ? '' : version,
    language === documentationRegistry.defaultLanguage ? '' : language,
  ].filter(Boolean);
}

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

const productHtml = await readFile(path.join(outputRoot, 'index.html'), 'utf8');
const productFragments = [
  'A3S OS',
  '企业级 AI 操作系统',
  '自主工作流编排',
  '异构智能体工厂',
  '统一网关',
  'A3S Work',
  '模块架构',
];
for (const fragment of productFragments) {
  if (!productHtml.includes(fragment)) {
    throw new Error(`Product home is missing ${fragment}`);
  }
}
if (/[—–]/u.test(productHtml)) {
  throw new Error('Product home contains a forbidden dash character');
}
if (productHtml.includes('A3S Cloud')) {
  throw new Error('Product home contains retired A3S Cloud branding');
}

const documentationPages = [];
for (const version of documentationRegistry.versions) {
  for (const language of documentationLanguages) {
    const prefix = documentationPrefix(version, language);
    const source = await readFile(
      path.join(documentationRoot, version, language, 'index.mdx'),
      'utf8',
    );
    const title = source.match(/^title:\s*["']?(.+?)["']?\s*$/m)?.[1];
    if (!title) {
      throw new Error(`${version}/${language}/index.mdx has no title`);
    }
    documentationPages.push({
      file: path.posix.join(...prefix, 'index.html'),
      language,
      logoHome: `${base}${prefix.join('/')}/`,
      title,
    });
  }
}

for (const page of documentationPages) {
  const html = await readFile(path.join(outputRoot, page.file), 'utf8');
  const expectedFragments = [
    `<html lang="${page.language}">`,
    `<title>${page.title} - A3S OS Docs</title>`,
    `href="${page.logoHome}" class="rp-nav__title__link`,
    ...documentationRegistry.languages.map(({ label }) => `>${label}<`),
    ...documentationRegistry.versions.map((version) => `>${version}<`),
    'rel="alternate"',
  ];
  for (const fragment of expectedFragments) {
    if (!html.includes(fragment)) {
      throw new Error(`${page.file} is missing ${fragment}`);
    }
  }
}

const searchIndexFiles = (
  await collectFiles(path.join(outputRoot, 'docs', 'static'), (name) =>
    name.startsWith('search_index.'),
  )
).map((file) => path.basename(file));
for (const version of documentationRegistry.versions) {
  for (const language of documentationLanguages) {
    const group = `${version.replaceAll('.', '_')}.${language}`;
    if (
      !searchIndexFiles.some((file) =>
        file.startsWith(`search_index.${group}.`),
      )
    ) {
      throw new Error(`Missing documentation search index for ${group}`);
    }
  }
}

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
