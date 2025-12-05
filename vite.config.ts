import { resolve } from 'node:path'
import { env } from 'node:process'

import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'
import { defineConfig } from 'vite'

const host = env.TAURI_DEV_HOST

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), UnoCSS()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  build: {
    chunkSizeWarningLimit: 500,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules')) {
            if (id.includes('pixi.js')) return 'pixi'
            if (id.includes('pixi-live2d-display')) return 'live2d-core'
            if (id.includes('@ant-design/icons-vue')) return 'antd-icons'
            if (id.includes('ant-design-vue/es/flex')) return 'antd-flex'
            if (id.includes('ant-design-vue/es/space')) return 'antd-space'
            if (id.includes('ant-design-vue/es/select')) return 'antd-select'
            if (id.includes('ant-design-vue/es/switch')) return 'antd-switch'
            if (id.includes('ant-design-vue/es/card')) return 'antd-card'
            if (id.includes('ant-design-vue/es/popconfirm')) return 'antd-popconfirm'
            if (id.includes('ant-design-vue/es/modal')) return 'antd-modal'
            if (id.includes('ant-design-vue/es/message')) return 'antd-message'
            if (id.includes('ant-design-vue/es/config-provider')) return 'antd-config'
            if (id.includes('ant-design-vue')) return 'antd'
            if (
              id.includes('node_modules/vue/')
              || id.includes('node_modules/vue/dist/')
              || id.includes('node_modules/@vue/')
            ) {
              return 'vue'
            }
          }
        },
      },
    },
  },
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },
}))
