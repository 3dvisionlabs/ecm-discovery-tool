// ESLint flat config (ESLint 9+).
import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: ['**/dist/', 'target/', 'app/', 'node_modules/', 'bugs/', 'references/'],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['ui/src/**/*.ts'],
    languageOptions: {
      globals: { ...globals.browser },
    },
  },
  {
    files: ['scripts/**/*.ts', 'ui/*.config.ts'],
    languageOptions: {
      globals: { ...globals.node },
    },
  },
);
