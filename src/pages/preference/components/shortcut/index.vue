<script setup lang="ts">
import { storeToRefs } from 'pinia'

import ProList from '@/components/pro-list/index.vue'
import ProShortcut from '@/components/pro-shortcut/index.vue'
import { useTauriShortcut } from '@/composables/useTauriShortcut'
import { toggleWindowVisible } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useShortcutStore } from '@/stores/shortcut.ts'

const shortcutStore = useShortcutStore()
const { visibleCat, visiblePreference, mirrorMode, penetrable, alwaysOnTop } = storeToRefs(shortcutStore)
const catStore = useCatStore()

useTauriShortcut(visibleCat, () => {
  catStore.visible = !catStore.visible
})

useTauriShortcut(visiblePreference, () => {
  toggleWindowVisible('preference')
})

useTauriShortcut(mirrorMode, () => {
  catStore.mirrorMode = !catStore.mirrorMode
})

useTauriShortcut(penetrable, () => {
  catStore.penetrable = !catStore.penetrable
})

useTauriShortcut(alwaysOnTop, () => {
  catStore.alwaysOnTop = !catStore.alwaysOnTop
})
</script>

<template>
  <!-- 快捷键 shortcut key  -->
  <ProList title="phím tắt">
    <!-- 切换猫咪窗口的显示与隐藏。  Toggle the display and hide of the cat window. -->
    <!--  打开猫咪  Open the cat -->
    <ProShortcut
      v-model="shortcutStore.visibleCat"
      description="Chuyển đổi màn hình và ẩn của cửa sổ mèo."
      title="Mở con mèo"
    />
    <!-- 切换偏好设置窗口的显示与隐藏。Toggle the display and hide of the preferences window. -->
    <!-- 打开偏好设置   Turn on Preferences -->
    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      description="Chuyển đổi màn hình và ẩn của cửa sổ Tùy chọn."
      title="Bật sở thích"
    />
    <!-- 切换猫咪的镜像模式。Switch the cat's mirror mode. -->
    <!--  镜像模式  Mirror mode -->
    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      description="Chuyển đổi chế độ gương của mèo."
      title="Chế độ gương"
    />
    <!-- 切换猫咪窗口是否可穿透。 Switch whether the cat window is penetrating. -->
    <!-- 窗口穿透   Window penetration -->
    <ProShortcut
      v-model="shortcutStore.penetrable"
      description="Chuyển đổi cho dù cửa sổ mèo có thâm nhập không."
      title="Thâm nhập cửa sổ"
    />
    <!-- 切换猫咪窗口是否置顶。 Toggle whether the cat window is topped. 窗口置顶 Top window -->
    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      description="Chuyển đổi xem cửa sổ mèo có đứng đầu không."
      title="Cửa sổ trên cùng"
    />
  </ProList>
</template>
