<script setup lang="ts">
import type { SelectProps } from 'ant-design-vue'

import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
import { Select, Switch } from 'ant-design-vue'
import { watch } from 'vue'
import { useI18n } from 'vue-i18n'

import MacosPermissions from './components/macos-permissions/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useI18n as useI18nPlugin } from '@/plugins/i18n'
import { useAppStore } from '@/stores/app'
import { useGeneralStore } from '@/stores/general'

const { t } = useI18n()
const { setLocale } = useI18nPlugin()
const generalStore = useGeneralStore()
const appStore = useAppStore()

const languageOptions = [
  { label: '中文', value: 'zh' },
  { label: 'English', value: 'en' },
]

function changeLanguage(value: SelectProps['value']) {
  if (value === 'zh' || value === 'en') {
    setLocale(value)
  }
}

watch(() => generalStore.autostart, async (value) => {
  const enabled = await isEnabled()

  if (value && !enabled) {
    return enable()
  }

  if (!value && enabled) {
    disable()
  }
}, { immediate: true })

watch(() => appStore.locale, (newlocale) => {
  setLocale(newlocale)
}, { immediate: true })
</script>

<template>
  <MacosPermissions />

  <ProList :title="t('general.appSettings')">
    <ProListItem :title="t('general.autostart')">
      <Switch v-model:checked="generalStore.autostart" />
    </ProListItem>

    <ProListItem title="语言 / Language">
      <Select
        v-model:value="appStore.locale"
        :options="languageOptions"
        style="width: 120px"
        @change="changeLanguage"
      />
    </ProListItem>
  </ProList>

  <ProList :title="t('general.updateSettings')">
    <ProListItem :title="t('general.autoCheckUpdate')">
      <Switch v-model:checked="generalStore.autoCheckUpdate" />
    </ProListItem>
  </ProList>
</template>
