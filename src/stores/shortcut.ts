import type { ShortcutHandler } from '@tauri-apps/plugin-global-shortcut'

import { isRegistered, register } from '@tauri-apps/plugin-global-shortcut'
import { defineStore } from 'pinia'
import { ref } from 'vue'

import { useCatStore } from './cat.ts'

import { hideWindow, showWindow } from '@/plugins/window.ts'

export type HotKey = 'visibleCat' | 'mirrorMode' | 'penetrable'

export const useShortcutStore = defineStore('shortcut', () => {
  const enabled = ref(false)
  const hotKeys = ref<Record<HotKey, string>>({
    visibleCat: '',
    mirrorMode: '',
    penetrable: '',
  })
  const handlers = ref<Record<HotKey, ShortcutHandler>>({
    visibleCat(event) {
      if (!enabled.value) return
      if (event.state === 'Released') {
        const catStore = useCatStore()
        catStore.visible = !catStore.visible
        catStore.visible ? showWindow('main') : hideWindow('main')
      }
    },
    mirrorMode(event) {
      if (!enabled.value) return
      if (event.state === 'Released') {
        const catStore = useCatStore()
        catStore.mirrorMode = !catStore.mirrorMode
      }
    },
    penetrable(event) {
      if (!enabled.value) return
      if (event.state === 'Released') {
        const catStore = useCatStore()
        catStore.penetrable = !catStore.penetrable
      }
    },
  })

  async function loadShortcuts() {
    const keys = Object.keys(hotKeys.value) as HotKey[]
    for (const key of keys) {
      const hotKey = hotKeys.value[key]
      if (hotKey && !(await isRegistered(hotKey))) {
        register(hotKey, handlers.value[key])
      }
    }
  }

  return {
    enabled,
    hotKeys,
    handlers,
    loadShortcuts,
  }
})
