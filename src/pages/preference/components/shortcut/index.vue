<script setup lang="ts">
import { storeToRefs } from 'pinia'

import ProList from '@/components/pro-list/index.vue'
import ProShortcut from '@/components/pro-shortcut/index.vue'
import { useTauriKeyPress } from '@/composables/useTauriKeyPress'
import { useCatStore } from '@/stores/cat'
import { useShortcutStore } from '@/stores/shortcut.ts'

const shortcutStore = useShortcutStore()
const { visibleCat, mirrorMode, penetrable, alwaysOnTop } = storeToRefs(shortcutStore)
const catStore = useCatStore()

useTauriKeyPress(visibleCat, () => {
  catStore.visible = !catStore.visible
})

useTauriKeyPress(mirrorMode, () => {
  catStore.mirrorMode = !catStore.mirrorMode
})

useTauriKeyPress(penetrable, () => {
  catStore.penetrable = !catStore.penetrable
})

useTauriKeyPress(alwaysOnTop, () => {
  catStore.alwaysOnTop = !catStore.alwaysOnTop
})
</script>

<template>
  <ProList title="快捷键列表">
    <ProShortcut
      v-model="shortcutStore.visibleCat"
      title="显示与隐藏猫咪窗口"
    />

    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      title="显示与隐藏偏好设置窗口"
    />

    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      title="镜像模式"
    />

    <ProShortcut
      v-model="shortcutStore.penetrable"
      title="窗口穿透"
    />

    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      title="窗口置顶"
    />
  </ProList>
</template>
