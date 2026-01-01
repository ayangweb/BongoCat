<script setup lang="ts">
import { Select, SelectOption } from 'ant-design-vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()
</script>

<template>
  <ProListItem
    :description="$t('pages.preference.cat.hints.position')"
    :title="$t('pages.preference.cat.labels.position')"
  >
    <Select v-model:value="catStore.window.position">
      <SelectOption value="bottomRight">
        {{ $t('pages.preference.cat.options.bottomRight') }}
      </SelectOption>
      <SelectOption value="bottomLeft">
        {{ $t('pages.preference.cat.options.bottomLeft') }}
      </SelectOption>
      <SelectOption value="topLeft">
        {{ $t('pages.preference.cat.options.topLeft') }}
      </SelectOption>
      <SelectOption value="topRight">
        {{ $t('pages.preference.cat.options.topRight') }}
      </SelectOption>
    </Select>
  </ProListItem>
</template>
