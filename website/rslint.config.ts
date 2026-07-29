import {
  defineConfig,
  js,
  reactHooksPlugin,
  reactPlugin,
  ts,
} from '@rslint/core';

export default defineConfig([
  { ignores: ['doc_build/**'] },
  js.configs.recommended,
  ts.configs.recommended,
  reactPlugin.configs.recommended,
  reactHooksPlugin.configs.recommended,
]);
