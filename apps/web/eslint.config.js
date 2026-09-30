import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import config from './svelte.config.js';
export default ts.config(
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs.recommended,
  { languageOptions: { globals: { ...globals.browser, ...globals.node } } },
  {
    files: ['**/*.svelte'],
    languageOptions: { parserOptions: { parser: ts.parser, svelteConfig: config } }
  }, // The app is served at the origin root in both node and desktop modes; /api links are backend downloads.
  { rules: { 'svelte/no-navigation-without-resolve': 'off' } },
  { ignores: ['.svelte-kit/**', 'build/**', 'node_modules/**', 'test-results/**'] }
);
