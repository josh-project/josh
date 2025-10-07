import js from '@eslint/js';
import tseslint from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';
import reactPlugin from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

const reactRecommended = {
  ...reactPlugin.configs.flat.recommended,
  files: ['**/*.{jsx,tsx}'],
  settings: {
    react: {
      version: 'detect',
    },
  },
};

const reactHooksRecommended = {
  ...reactHooks.configs.flat.recommended,
  files: ['**/*.{jsx,tsx}'],
};

export default [
  js.configs.recommended,
  ...tseslint.configs['flat/recommended'],
  {
    files: ['**/*.{ts,tsx,js,jsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 'latest',
        sourceType: 'module',
        ecmaFeatures: {
          jsx: true,
        },
      },
    },
  },
  reactRecommended,
  reactHooksRecommended,
];
