import svelte from 'eslint-plugin-svelte';
import svelteParser from 'svelte-eslint-parser';
import ts from 'typescript-eslint';

export default [
  ...ts.configs.recommended,
  ...svelte.configs['flat/recommended'],
  {
    files: ['**/*.svelte'],
    languageOptions: {
      parser: svelteParser,
      parserOptions: {
        parser: ts.parser,
        extraFileExtensions: ['.svelte']
      }
    },
    rules: {
      'svelte/no-navigation-without-resolve': 'off',
      'svelte/prefer-svelte-reactivity': 'off'
    }
  },
  {
    ignores: ['build/', '.svelte-kit/', 'src-tauri/target/']
  }
];
