<script setup lang="ts">
import { Select } from 'antdv-next'

import type { Language } from '@/stores/general'

import ProListItem from '@/components/pro-list-item/index.vue'
import { LANGUAGE } from '@/constants'
import { useGeneralStore } from '@/stores/general'

const generalStore = useGeneralStore()

const options: Array<{ label: string, value: Language }> = [
  { label: '简体中文', value: LANGUAGE.ZH_CN },
  { label: '繁體中文', value: LANGUAGE.ZH_TW },
  { label: 'English', value: LANGUAGE.EN_US },
  { label: 'Tiếng Việt', value: LANGUAGE.VI_VN },
  { label: 'Português', value: LANGUAGE.PT_BR },
]
</script>

<template>
  <ProListItem :title="$t('pages.preference.general.labels.language')">
    <Select
      v-model:value="generalStore.appearance.language"
      :options="options"
    />
  </ProListItem>
</template>
