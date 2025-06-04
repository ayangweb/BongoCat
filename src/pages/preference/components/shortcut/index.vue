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
  <ProList title="快捷键">
    <ProShortcut
      v-model="shortcutStore.visibleCat"
      description="显示或隐藏猫咪窗口"
      title="打开猫咪窗口"
    />

    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      description="显示或隐藏偏好设置窗口"
      title="打开偏好设置窗口"
    />

    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      description="开启或关闭猫咪镜像模式"
      title="猫咪镜像模式"
    />

    <ProShortcut
      v-model="shortcutStore.penetrable"
      description="开启或关闭猫咪窗口穿透"
      title="猫咪窗口穿透"
    />

    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      description="开启或关闭猫咪窗口置顶"
      title="猫咪窗口置顶"
    />
  </ProList>
</template>
