import React from 'react';
import { render, screen } from '@testing-library/react';
import { afterEach, vi } from 'vitest';
import App from './App';

afterEach(() => {
  vi.restoreAllMocks();
});

test('renders repo selector heading', async () => {
  const mockFetch = vi.fn(() =>
    Promise.resolve({
      text: () => Promise.resolve('http://example.com'),
    }),
  ) as unknown as typeof fetch;
  vi.stubGlobal('fetch', mockFetch);

  render(<App />);

  expect(await screen.findByText(/Select repo/i)).toBeInTheDocument();
});
