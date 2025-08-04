<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core'
import { resolveResource } from '@tauri-apps/api/path'
import { openPath } from '@tauri-apps/plugin-opener'
import { Button, Select, SelectOption } from 'ant-design-vue'
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useLocale } from '@/composables/useLocale'

// Reactive to save icon path
const languageIcon = ref('')
const { t } = useI18n()
const { generalStore, locales } = useLocale()

// Grab icon PNG from assets/locales
async function updateLanguageIcon() {
  const file = generalStore.isDark ? 'languages-white.svg' : 'languages-black.svg'
  const path = await resolveResource(`assets/locales/${file}`)
  languageIcon.value = convertFileSrc(path)
}
// Open locales folder
async function openLocalesFolder() {
  const localesPath = await resolveResource('assets/locales')
  await openPath(localesPath)
}

onMounted(() => {
  updateLanguageIcon()

  // Watch theme change to update icon
  watch(
    () => generalStore.isDark,
    () => {
      updateLanguageIcon()
    },
    { immediate: true }, // 👈 thêm dòng này
  )
})
</script>

<template>
  <ProListItem :title="t('general.languages.title')">
    <!--
      ⚠️ Do NOT remove or change this fixed width!
      If the Select shrinks to fit short labels (e.g., "中文"),
      it will trigger layout reflow and freeze the UI when switching languages.
      Keep a fixed width or a large enough min-width to avoid this bug.
    -->
    <img
      v-if="languageIcon"
      class="h-5 w-5 cursor-pointer"
      :src="languageIcon"
      :title="t('general.languages.customize')"
      @click="openLocalesFolder"
    >
    <div class="flex flex-1 items-center justify-end gap-3">
      <div class="flex items-center justify-end gap-1">
        <div class="text-xs text-gray-400">
          {{ t('general.languages.customize') }}
        </div>
        <Button
          class="flex items-center gap-1"
          @click="openLocalesFolder"
        >
          <span>{{ t('general.languages.openFolder') }}</span>
          <i class="i-iconamoon:link-external-bold text-4" />
        </Button>
      </div>

      <Select
        v-model:value="generalStore.locale"
        style="width: 150px"
      >
        <SelectOption
          v-for="loc in locales"
          :key="loc.code"
          :value="loc.code"
        >
          {{ loc.name }}
        </SelectOption>
      </Select>
    </div>
  </ProListItem>
</template>
