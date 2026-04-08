<script setup lang="ts">
import { storeToRefs } from 'pinia'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import Shortcut from '@/components/shortcut/index.vue'
import { useKeyPress } from '@/composables/useKeyPress'
import { toggleWindowVisible } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useShortcutStore } from '@/stores/shortcut.ts'

const shortcutStore = useShortcutStore()
const { visibleCat, visiblePreference, mirrorMode, penetrable, alwaysOnTop } = storeToRefs(shortcutStore)
const catStore = useCatStore()

useKeyPress(visibleCat, () => {
  catStore.window.visible = !catStore.window.visible
})

useKeyPress(visiblePreference, () => {
  toggleWindowVisible('preference')
})

useKeyPress(mirrorMode, () => {
  catStore.model.mirror = !catStore.model.mirror
})

useKeyPress(penetrable, () => {
  catStore.window.passThrough = !catStore.window.passThrough
})

useKeyPress(alwaysOnTop, () => {
  catStore.window.alwaysOnTop = !catStore.window.alwaysOnTop
})
</script>

<template>
  <ProList :title="$t('pages.preference.shortcut.title')">
    <ProListItem
      :description="$t('pages.preference.shortcut.hints.toggleCat')"
      :title="$t('pages.preference.shortcut.labels.toggleCat')"
    >
      <Shortcut v-model="shortcutStore.visibleCat" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.shortcut.hints.togglePreferences')"
      :title="$t('pages.preference.shortcut.labels.togglePreferences')"
    >
      <Shortcut v-model="shortcutStore.visiblePreference" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.shortcut.hints.mirrorMode')"
      :title="$t('pages.preference.shortcut.labels.mirrorMode')"
    >
      <Shortcut v-model="shortcutStore.mirrorMode" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.shortcut.hints.passThrough')"
      :title="$t('pages.preference.shortcut.labels.passThrough')"
    >
      <Shortcut v-model="shortcutStore.penetrable" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.shortcut.hints.alwaysOnTop')"
      :title="$t('pages.preference.shortcut.labels.alwaysOnTop')"
    >
      <Shortcut v-model="shortcutStore.alwaysOnTop" />
    </ProListItem>
  </ProList>
</template>
