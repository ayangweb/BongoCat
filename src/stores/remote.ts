import { message } from 'ant-design-vue'
import { defineStore } from 'pinia'
import { ref } from 'vue'

import { useModelStore } from './model'

export const useRemoteStore = defineStore('remote', () => {
  const serverIp = ref('')
  const isConnected = ref(false)
  const ws = ref<WebSocket>()

  const connect = () => {
    if (isConnected.value || !serverIp.value) return

    ws.value = new WebSocket(`ws://${serverIp.value}:3000`)

    ws.value.onopen = () => {
      isConnected.value = true
    }
    ws.value.onclose = () => {
      isConnected.value = false
    }
    ws.value.onerror = () => {
      message.error('无法连接至服务器')
    }
    ws.value.onmessage = (event) => {
      const { type, data } = JSON.parse(event.data)

      switch (type) {
        case 'live2d':
          useModelStore().init(data)
          break

        default:
          break
      }
    }
  }

  const disconnect = () => {
    ws.value?.close()
  }

  return {
    ws,
    serverIp,
    isConnected,
    connect,
    disconnect,
  }
}, {
  tauri: {
    filterKeys: ['serverIp'],
    filterKeysStrategy: 'pick',
  },
})
