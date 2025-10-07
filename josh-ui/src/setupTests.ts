// jest-dom adds custom jest matchers for asserting on DOM nodes.
// allows you to do things like:
// expect(element).toHaveTextContent(/react/i)
// learn more: https://github.com/testing-library/jest-dom
import '@testing-library/jest-dom';
import { vi } from 'vitest';

// The Monaco editor pulls in loader logic that depends on CommonJS.
// The UI tests don't exercise those components, so stub them out.
vi.mock('@monaco-editor/react', () => ({
  __esModule: true,
  default: () => null,
  MonacoDiffEditor: () => null,
  MonacoEditor: () => null,
}));
