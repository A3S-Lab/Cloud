import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const websiteRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const repositoryRoot = path.resolve(websiteRoot, '..');
const documentationRoot = path.join(websiteRoot, 'documentation');
const registryPath = path.join(documentationRoot, 'versions.json');

function fail(message) {
  throw new Error(`Documentation check failed: ${message}`);
}

async function isDirectory(candidate) {
  try {
    return (await stat(candidate)).isDirectory();
  } catch {
    return false;
  }
}

async function collectMarkdown(directory, root = directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const absolutePath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectMarkdown(absolutePath, root)));
    } else if (/\.mdx?$/.test(entry.name)) {
      files.push(path.relative(root, absolutePath).split(path.sep).join('/'));
    }
  }
  return files.sort();
}

const registryValue = JSON.parse(await readFile(registryPath, 'utf8'));
if (
  typeof registryValue !== 'object' ||
  registryValue === null ||
  typeof registryValue.default !== 'string' ||
  typeof registryValue.defaultLanguage !== 'string' ||
  !Array.isArray(registryValue.languages) ||
  !registryValue.languages.every(
    (language) =>
      typeof language === 'object' &&
      language !== null &&
      typeof language.id === 'string' &&
      typeof language.label === 'string',
  ) ||
  !Array.isArray(registryValue.versions) ||
  !registryValue.versions.every((version) => typeof version === 'string')
) {
  fail('versions.json is not a documentation registry');
}
const registry = registryValue;
const languages = registry.languages.map((language) => language.id);
if (languages.length < 2) {
  fail('at least two languages are required for a visible language switcher');
}
if (new Set(languages).size !== languages.length) {
  fail('versions.json contains duplicate languages');
}
if (languages[0] !== registry.defaultLanguage) {
  fail('the default language must be the first registered language');
}
if (registry.versions.length < 2) {
  fail('at least two versions are required for a visible version switcher');
}
if (new Set(registry.versions).size !== registry.versions.length) {
  fail('versions.json contains duplicate versions');
}
if (registry.versions[0] !== registry.default) {
  fail('the default version must be the first registered version');
}
if (!registry.versions.includes('next')) {
  fail('the next documentation channel is not registered');
}

const cargoManifest = await readFile(
  path.join(repositoryRoot, 'Cargo.toml'),
  'utf8',
);
const workspaceVersion = cargoManifest.match(
  /\[workspace\.package\][\s\S]*?^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"/m,
);
if (!workspaceVersion) fail('the workspace package version could not be read');
const expectedDefault = `v${workspaceVersion[1]}.${workspaceVersion[2]}`;
if (registry.default !== expectedDefault) {
  fail(
    `default ${registry.default} does not match workspace series ${expectedDefault}`,
  );
}

const versionDirectories = (
  await readdir(documentationRoot, { withFileTypes: true })
)
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();
const undeclaredVersions = versionDirectories.filter(
  (version) => !registry.versions.includes(version),
);
if (undeclaredVersions.length > 0) {
  fail(`unregistered version directories: ${undeclaredVersions.join(', ')}`);
}

for (const version of registry.versions) {
  const versionRoot = path.join(documentationRoot, version);
  if (!(await isDirectory(versionRoot))) {
    fail(`missing version directory: ${version}`);
  }

  const languageDirectories = (
    await readdir(versionRoot, { withFileTypes: true })
  )
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name);
  const undeclaredLanguages = languageDirectories.filter(
    (language) => !languages.includes(language),
  );
  if (undeclaredLanguages.length > 0) {
    fail(
      `${version} contains unregistered languages: ${undeclaredLanguages.join(', ')}`,
    );
  }

  const pagesByLanguage = new Map();
  for (const language of languages) {
    const languageRoot = path.join(versionRoot, language);
    if (!(await isDirectory(languageRoot))) {
      fail(`missing ${language} content for ${version}`);
    }
    pagesByLanguage.set(language, await collectMarkdown(languageRoot));
  }

  const referencePages = pagesByLanguage.get(languages[0]);
  if (!referencePages.includes('index.mdx')) {
    fail(`${version} does not contain a documentation home page`);
  }
  for (const language of languages.slice(1)) {
    const pages = pagesByLanguage.get(language);
    const missing = referencePages.filter((page) => !pages.includes(page));
    const extra = pages.filter((page) => !referencePages.includes(page));
    if (missing.length > 0 || extra.length > 0) {
      fail(
        `${version}/${language} is out of parity (missing: ${missing.join(', ') || 'none'}; extra: ${extra.join(', ') || 'none'})`,
      );
    }
  }
}

const preservationContracts = [
  {
    path: 'docs/architecture.md',
    required: [
      '### 2.1 Reference capability preservation register',
      'Organizations, projects, environments, identity, grants, REST/OpenAPI, TypeScript client, CLI, and Management MCP',
      'External sources, webhooks, reproducible builds, provenance, previews, monorepos, and imports',
      'Generic finite Tasks and ordinary application Services',
      'Runtime/Box provider lifecycle, isolation, mounts, outputs, checkpoints, and builds',
      'TokenHub-style private multi-provider model gateway',
      'Google AX-style isolated distributed Harness execution',
      'Security-operations correlation',
      '`HarnessInvocationProfile`',
      '`I0.6`',
    ],
  },
  {
    path: 'ROADMAP.md',
    required: [
      'external identity federation',
      'OIDC issuer/subject links',
      '`HarnessInvocationProfile`',
      '`I0.6`',
      'tenant-scoped security investigation',
    ],
  },
  {
    path: 'docs/development-plan.md',
    required: [
      'TokenHub-style private model gateway',
      'Google AX-style distributed Harness runtime',
      'Cross-layer security operations',
      '`HarnessInvocationProfile`',
      'optional independently certified `I0.6` protocol/channel profiles',
    ],
  },
  {
    path: 'docs/domain-model.md',
    required: [
      '| External identity link |',
      '| Harness invocation profile |',
      '| Security incident projection |',
    ],
  },
  {
    path: 'docs/inference-plan.md',
    required: [
      'The TokenHub reference inventory is preserved by outcome',
      '### I0.6: optional protocol and Provider-channel expansion',
    ],
  },
];

for (const contract of preservationContracts) {
  const content = await readFile(
    path.join(repositoryRoot, contract.path),
    'utf8',
  );
  const missing = contract.required.filter(
    (needle) => !content.includes(needle),
  );
  if (missing.length > 0) {
    fail(
      `${contract.path} lost capability-preservation invariants: ${missing.join(', ')}`,
    );
  }
}

console.log(
  `Verified ${registry.versions.length} documentation versions across ${languages.length} languages and ${preservationContracts.length} capability-preservation contracts.`,
);
