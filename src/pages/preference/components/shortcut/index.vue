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
  catStore.window.visible = !catStore.window.visible
})

useTauriShortcut(visiblePreference, () => {
  toggleWindowVisible('preference')
})

useTauriShortcut(mirrorMode, () => {
  catStore.model.mirror = !catStore.model.mirror
})

useTauriShortcut(penetrable, () => {
  catStore.window.passThrough = !catStore.window.passThrough
})

useTauriShortcut(alwaysOnTop, () => {
  catStore.window.alwaysOnTop = !catStore.window.alwaysOnTop
})
</script>

<template>
  <ProList :title="$t('pages.preference.shortcut.title')">
    <ProShortcut
      v-model="shortcutStore.visibleCat"
      :description="$t('pages.preference.shortcut.hints.toggleCat')"
      :title="$t('pages.preference.shortcut.labels.toggleCat')"
    />

    <ProShortcut
      v-model="shortcutStore.visiblePreference"
      :description="$t('pages.preference.shortcut.hints.togglePreferences')"
      :title="$t('pages.preference.shortcut.labels.togglePreferences')"
    />

    <ProShortcut
      v-model="shortcutStore.mirrorMode"
      :description="$t('pages.preference.shortcut.hints.mirrorMode')"
      :title="$t('pages.preference.shortcut.labels.mirrorMode')"
    />

    <ProShortcut
      v-model="shortcutStore.penetrable"
      :description="$t('pages.preference.shortcut.hints.passThrough')"
      :title="$t('pages.preference.shortcut.labels.passThrough')"
    />

    <ProShortcut
      v-model="shortcutStore.alwaysOnTop"
      :description="$t('pages.preference.shortcut.hints.alwaysOnTop')"
      :title="$t('pages.preference.shortcut.labels.alwaysOnTop')"
    />
  </ProList>
</template>
