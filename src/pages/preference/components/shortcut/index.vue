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
  <ProList title="Shortcut Keys">
    <ProShortcut
      v-model="shortcutStore.visibleCat"
      description="Toggle the display of the cat window"
      title="Open Cat"
    />

    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      description="Toggle the display of the preference window"
      title="Open Preferences"
    />

    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      description="Toggle mirror mode for the cat"
      title="Mirror Mode"
    />

    <ProShortcut
      v-model="shortcutStore.penetrable"
      description="Toggle whether the cat window is penetrable"
      title="Window Penetration"
    />

    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      description="Toggle whether the cat window is always on top"
      title="Always on Top"
    />
  </ProList>
</template>
