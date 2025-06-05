<script setup lang="ts">
import { storeToRefs } from 'pinia'

import ProList from '@/components/pro-list/index.vue'
import ProShortcut from '@/components/pro-shortcut/index.vue'
import { useTauriKeyPress } from '@/composables/useTauriKeyPress'
import { toggleWindowVisible } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useShortcutStore } from '@/stores/shortcut.ts'

const shortcutStore = useShortcutStore()
const { visibleCat, visiblePreference, mirrorMode, penetrable, alwaysOnTop } = storeToRefs(shortcutStore)
const catStore = useCatStore()

useTauriKeyPress(visibleCat, () => {
  catStore.visible = !catStore.visible
})

useTauriKeyPress(visiblePreference, () => {
  toggleWindowVisible('preference')
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
      description="切换猫咪窗口的显示与隐藏"
      title="打开猫咪"
    />

    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      description="切换偏好设置窗口的显示与隐藏"
      title="打开偏好设置"
    />

    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      description="切换猫咪的镜像模式"
      title="镜像模式"
    />

    <ProShortcut
      v-model="shortcutStore.penetrable"
      description="切换猫咪窗口是否可穿透"
      title="窗口穿透"
    />

    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      description="切换猫咪窗口是否置顶"
      title="窗口置顶"
    />
  </ProList>
</template>
