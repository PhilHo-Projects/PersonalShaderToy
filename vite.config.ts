import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 5199,
    strictPort: true,
    proxy: {
      '/api': 'http://localhost:4781',
    },
  },
});
