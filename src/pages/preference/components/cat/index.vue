<script setup lang="ts">
import { InputNumber, Slider, Switch } from 'ant-design-vue'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const { t } = useI18n()
const catStore = useCatStore()
</script>

<template>
  <ProList :title="t('modelSettings.title')">
    <ProListItem
      :description="t('modelSettings.mirrorModeDesc')"
      :title="t('modelSettings.mirrorMode')"
    >
      <Switch v-model:checked="catStore.mirrorMode" />
    </ProListItem>

    <ProListItem
      :description="t('modelSettings.singleModeDesc')"
      :title="t('modelSettings.singleMode')"
    >
      <Switch v-model:checked="catStore.singleMode" />
    </ProListItem>

    <ProListItem
      :description="t('modelSettings.mouseMirrorDesc')"
      :title="t('modelSettings.mouseMirror')"
    >
      <Switch v-model:checked="catStore.mouseMirror" />
    </ProListItem>
  </ProList>

  <ProList :title="t('windowSettings.title')">
    <ProListItem
      :description="t('windowSettings.penetrableDesc')"
      :title="t('windowSettings.penetrable')"
    >
      <Switch v-model:checked="catStore.penetrable" />
    </ProListItem>

    <ProListItem
      :description="t('windowSettings.alwaysOnTopDesc')"
      :title="t('windowSettings.alwaysOnTop')"
    >
      <Switch v-model:checked="catStore.alwaysOnTop" />
    </ProListItem>

    <ProListItem
      :description="t('windowSettings.sizeDesc')"
      :title="t('windowSettings.size')"
    >
      <InputNumber
        v-model:value="catStore.scale"
        class="w-28"
        :min="1"
      >
        <template #addonAfter>
          %
        </template>
      </InputNumber>
    </ProListItem>

    <ProListItem
      :title="t('windowSettings.opacity')"
      vertical
    >
      <Slider
        v-model:value="catStore.opacity"
        class="m-0!"
        :max="100"
        :min="10"
        :tip-formatter="(value) => `${value}%`"
      />
    </ProListItem>
  </ProList>
</template>
