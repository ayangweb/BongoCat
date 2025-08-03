<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProShortcut from '@/components/pro-shortcut/index.vue'
import { useTauriShortcut } from '@/composables/useTauriShortcut'
import { toggleWindowVisible } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useShortcutStore } from '@/stores/shortcut.ts'

const { t } = useI18n()
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
  <ProList :title="t('shortcuts.title')">
    <ProShortcut
      v-model="shortcutStore.visibleCat"
      :description="t('shortcuts.items.visibleCat.desc')"
      :title="t('shortcuts.items.visibleCat.title')"
    />

    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      :description="t('shortcuts.items.visiblePreference.desc')"
      :title="t('shortcuts.items.visiblePreference.title')"
    />

    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      :description="t('shortcuts.items.mirrorMode.desc')"
      :title="t('shortcuts.items.mirrorMode.title')"
    />

    <ProShortcut
      v-model="shortcutStore.penetrable"
      :description="t('shortcuts.items.penetrable.desc')"
      :title="t('shortcuts.items.penetrable.title')"
    />

    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      :description="t('shortcuts.items.alwaysOnTop.desc')"
      :title="t('shortcuts.items.alwaysOnTop.title')"
    />
  </ProList>
</template>
