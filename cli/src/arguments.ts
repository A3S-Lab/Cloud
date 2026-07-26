import { usageError } from './errors';

export interface ParsedArguments {
  positionals: string[];
  url?: string;
  organizationId?: string;
  projectId?: string;
  environmentId?: string;
  output?: string;
  timeoutMs?: string;
  help: boolean;
  version: boolean;
}

type ValueOption = Exclude<keyof ParsedArguments, 'help' | 'positionals' | 'version'>;

const VALUE_OPTIONS: Readonly<Record<string, ValueOption>> = {
  '--url': 'url',
  '--organization': 'organizationId',
  '--project': 'projectId',
  '--environment': 'environmentId',
  '--output': 'output',
  '--timeout': 'timeoutMs',
};

export function parseArguments(argv: readonly string[]): ParsedArguments {
  const parsed: ParsedArguments = {
    positionals: [],
    help: false,
    version: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--help' || argument === '-h') {
      parsed.help = true;
      continue;
    }
    if (argument === '--version' || argument === '-V') {
      parsed.version = true;
      continue;
    }
    if (argument === '--token' || argument.startsWith('--token=')) {
      throw usageError('API tokens are accepted only through A3S_CLOUD_TOKEN');
    }
    if (argument.startsWith('-')) {
      const separator = argument.indexOf('=');
      const name = separator === -1 ? argument : argument.slice(0, separator);
      const key = VALUE_OPTIONS[name];
      if (!key) {
        throw usageError(`unknown option ${JSON.stringify(name)}`);
      }
      if (parsed[key] !== undefined) {
        throw usageError(`option ${name} may be specified only once`);
      }
      const inlineValue = separator === -1 ? undefined : argument.slice(separator + 1);
      const value = inlineValue ?? argv[index + 1];
      if (!value || (inlineValue === undefined && value.startsWith('-'))) {
        throw usageError(`option ${name} requires a value`);
      }
      parsed[key] = value;
      if (inlineValue === undefined) {
        index += 1;
      }
      continue;
    }
    parsed.positionals.push(argument);
  }

  return parsed;
}
