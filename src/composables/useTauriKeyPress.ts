import type { ShortcutHandler } from '@tauri-apps/plugin-global-shortcut'
import type { Ref } from 'vue'

import {
  isRegistered,
  register,
  unregister,
} from '@tauri-apps/plugin-global-shortcut'
import { ref, watch } from 'vue'

export function useTauriKeyPress(shortcut: Ref<string, string>, callback: ShortcutHandler) {
  const oldShortcut = ref(shortcut.value)

  watch(shortcut, async (value) => {
    if (oldShortcut.value) {
      const registered = await isRegistered(oldShortcut.value)

      if (registered) {
        await unregister(oldShortcut.value)
      }
    }

    if (!value) return

    await register(value, (event) => {
      if (event.state === 'Released') return

      callback(event)
    })

    oldShortcut.value = value
  }, { immediate: true })
}
