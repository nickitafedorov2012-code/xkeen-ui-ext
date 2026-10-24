import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5175,
    proxy: {
      // XR_API_TARGET позволяет дев-серверу проксировать API на реальный роутер:
      // XR_API_TARGET=http://192.168.2.1:1001 npm run dev
      '/api': process.env.XR_API_TARGET || 'http://127.0.0.1:1001',
    },
  },
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 500,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom'],
        },
      },
    },
  },
  // @ts-ignore vitest config
  test: {
    environment: 'happy-dom',
  },
})
