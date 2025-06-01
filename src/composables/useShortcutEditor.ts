import type { HotKeys } from '@/stores/shortcut.ts'

import { isRegistered, register, unregister } from '@tauri-apps/plugin-global-shortcut'
import { useEventListener, useMagicKeys } from '@vueuse/core'
import { message } from 'ant-design-vue'
import { computed, ref } from 'vue'

import { useShortcutStore } from '@/stores/shortcut.ts'
import { isModifierKey, normalizeKey } from '@/utils/keyboard.ts'

export function useShortcutEditor(hotKey: keyof HotKeys, handler: () => void) {
  const shortcutStore = useShortcutStore()

  const isEditing = ref(false)
  const pressedKey = ref<string[]>([])

  function onEdit() {
    if (isEditing.value) return

    isEditing.value = true
    pressedKey.value = []
  }

  const { current } = useMagicKeys()
  useEventListener('keydown', (event) => {
    if (!isEditing.value) return
    event.preventDefault()
    event.stopPropagation()

    const keys = Array.from(current)
    const modifiers = keys.filter(k => isModifierKey(k))
    const nonModifiers = keys.filter(k => !isModifierKey(k))
    pressedKey.value = [...modifiers, ...nonModifiers].map(k => normalizeKey(k))
  })

  async function onSave() {
    if (!isEditing.value) return

    const oldShortcut = shortcutStore.hotKeys[hotKey]
    const newShortcut = pressedKey.value.join('+')

    if (!newShortcut) {
      if (oldShortcut && await isRegistered(oldShortcut)) {
        await unregister(oldShortcut)
      }
      shortcutStore.hotKeys[hotKey] = newShortcut
      isEditing.value = false
      return
    }

    if (await isRegistered(newShortcut)) {
      message.warn('该快捷键已被注册')
      return
    }

    if (oldShortcut) {
      await unregister(oldShortcut)
    }

    await register(newShortcut, (event) => {
      if (!shortcutStore.enabled) return

      if (event.state === 'Released') {
        handler()
      }
    })

    shortcutStore.hotKeys[hotKey] = newShortcut
    isEditing.value = false
  }

  return {
    isEditing,
    pressedKey,
    onEdit,
    onSave,
    hotKey: computed(() => shortcutStore.hotKeys[hotKey]),
  }
}
