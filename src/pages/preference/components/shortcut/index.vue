<script setup lang="ts">
import { isRegistered, register, unregister } from '@tauri-apps/plugin-global-shortcut'
import { useEventListener, useMagicKeys } from '@vueuse/core'
import { message, Switch } from 'ant-design-vue'
import { ref } from 'vue'

import HotKey from './components/hot-key/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { hideWindow, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat.ts'
import { useShortcutStore } from '@/stores/shortcut.ts'
import { isModifierKey, normalizeKey } from '@/utils/keyboard.ts'

const catStore = useCatStore()
const shortcutStore = useShortcutStore()

const isEditing = ref(false)
const pressedKey = ref<string[]>([])

async function onSave() {
  if (!isEditing.value) return

  const oldShortcut = shortcutStore.hosKeys.visibleCat
  const newShortcut = pressedKey.value.join('+')

  if (!newShortcut) {
    if (oldShortcut && await isRegistered(oldShortcut)) {
      await unregister(oldShortcut)
    }
    shortcutStore.hosKeys.visibleCat = newShortcut
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
      catStore.visible = !catStore.visible
      catStore.visible ? showWindow('main') : hideWindow('main')
    }
  })

  shortcutStore.hosKeys.visibleCat = newShortcut
  isEditing.value = false
}

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
</script>

<template>
  <ProList title="全局快捷键">
    <ProListItem title="是否启动">
      <Switch v-model:checked="shortcutStore.enabled" />
    </ProListItem>
  </ProList>

  <ProList title="快捷键列表">
    <ProListItem title="隐藏猫咪">
      <HotKey
        :hot-key="shortcutStore.hosKeys.visibleCat"
        :is-editing="isEditing"
        :pressed-key="pressedKey"
        @edit="onEdit"
        @save="onSave"
      />
    </ProListItem>
  </ProList>
</template>
