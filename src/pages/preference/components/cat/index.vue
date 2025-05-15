<script setup lang="ts">
import { Select, Slider, Switch } from 'ant-design-vue'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const { t } = useI18n()
const catStore = useCatStore()

const modeList = [
  {
    label: t('mode.standard'),
    value: 'standard',
  },
  {
    label: t('mode.keyboard'),
    value: 'keyboard',
  },
]

function scaleFormatter(value?: number) {
  return value === 100 ? t('window.size.default') : t('common.percent', { value })
}

function opacityFormatter(value?: number) {
  return t('common.percent', { value })
}
</script>

<template>
  <ProList :title="t('mode.title')">
    <ProListItem :title="t('mode.select')">
      <Select
        v-model:value="catStore.mode"
        :options="modeList"
        :title="t('mode.select')"
      />
    </ProListItem>
  </ProList>

  <ProList :title="t('window.title')">
    <ProListItem
      :description="t('window.penetrable.description')"
      :title="t('window.penetrable.title')"
    >
      <Switch v-model:checked="catStore.penetrable" />
    </ProListItem>

    <ProListItem
      :description="t('window.size.description')"
      :title="t('window.size.title')"
      vertical
    >
      <Slider
        v-model:value="catStore.scale"
        class="m-0!"
        :max="150"
        :min="50"
        :tip-formatter="scaleFormatter"
      />
    </ProListItem>

    <ProListItem
      :title="t('window.opacity.title')"
      vertical
    >
      <Slider
        v-model:value="catStore.opacity"
        class="m-0!"
        :tip-formatter="opacityFormatter"
      />
    </ProListItem>

    <ProListItem :title="t('window.mirror.title')">
      <Switch v-model:checked="catStore.mirrorMode" />
    </ProListItem>
  </ProList>
</template>
