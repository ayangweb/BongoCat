<script setup lang="ts">
import { isRegistered, register, unregister } from '@tauri-apps/plugin-global-shortcut'
import { message, Switch } from 'ant-design-vue'

import HotKey from './components/hot-key/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { hideWindow, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat.ts'
import { useShortcutStore } from '@/stores/shortcut.ts'

const catStore = useCatStore()
const shortcutStore = useShortcutStore()

async function onChangeShortcut(shortcut: string) {
  const oldShortcut = shortcutStore.hosKeys.visibleCat

  if (!shortcut) {
    if (oldShortcut && await isRegistered(oldShortcut)) {
      await unregister(oldShortcut)
    }
    shortcutStore.hosKeys.visibleCat = shortcut
    return
  }

  if (await isRegistered(shortcut)) {
    message.warn('该快捷键已被注册')
    return
  }

  if (oldShortcut) {
    await unregister(oldShortcut)
  }

  await register(shortcut, (event) => {
    if (!shortcutStore.enabled) return

    if (event.state === 'Released') {
      catStore.visible = !catStore.visible
      catStore.visible ? showWindow('main') : hideWindow('main')
    }
  })

  shortcutStore.hosKeys.visibleCat = shortcut
}
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
        @change-shortcut="onChangeShortcut"
      />
    </ProListItem>
  </ProList>
</template>
