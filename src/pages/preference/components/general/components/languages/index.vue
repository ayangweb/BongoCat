<script setup lang="ts">
import { Select, SelectOption } from 'ant-design-vue'
import { useI18n } from 'vue-i18n'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useLocale } from '@/composables/useLocale'

const { t } = useI18n()
const { generalStore, locales } = useLocale()
</script>

<template>
  <ProListItem :title="t('general.languages.title')">
    <!--
      ⚠️ Do NOT remove or change this fixed width!
      If the Select shrinks to fit short labels (e.g., "中文"),
      it will trigger layout reflow and freeze the UI when switching languages.
      Keep a fixed width or a large enough min-width to avoid this bug.
    -->
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
  </ProListItem>
</template>
