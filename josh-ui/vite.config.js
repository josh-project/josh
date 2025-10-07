import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig(({ command }) => {
  const defaultPublicUrl = command === 'build' ? '/~/ui' : '/';
  const publicUrl = process.env.PUBLIC_URL ?? defaultPublicUrl;
  const base = publicUrl.endsWith('/') ? publicUrl : `${publicUrl}/`;

  return {
    base,
    plugins: [react()],
    build: {
      outDir: 'build', // CRA's default build output
    },
    test: {
      globals: true,
      environment: 'jsdom',
      setupFiles: './src/setupTests.ts',
      css: true,
      pool: 'threads',
    },
  };
});
