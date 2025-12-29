<script setup lang="ts">
import { Button, InputNumber, Popconfirm, Slider, Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'
import { useStatisticsStore } from '@/stores/statistics'
import { isWindows } from '@/utils/platform'

const catStore = useCatStore()
const statisticsStore = useStatisticsStore()

function handleReset() {
  statisticsStore.reset()
}
</script>

<template>
  <ProList :title="$t('pages.preference.cat.labels.modelSettings')">
    <ProListItem
      :description="$t('pages.preference.cat.hints.mirrorMode')"
      :title="$t('pages.preference.cat.labels.mirrorMode')"
    >
      <Switch v-model:checked="catStore.model.mirror" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.cat.hints.singleMode')"
      :title="$t('pages.preference.cat.labels.singleMode')"
    >
      <Switch v-model:checked="catStore.model.single" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.cat.hints.mouseMirror')"
      :title="$t('pages.preference.cat.labels.mouseMirror')"
    >
      <Switch v-model:checked="catStore.model.mouseMirror" />
    </ProListItem>

    <ProListItem
      v-if="isWindows"
      :description="$t('pages.preference.cat.hints.autoReleaseDelay')"
      :title="$t('pages.preference.cat.labels.autoReleaseDelay')"
    >
      <InputNumber
        v-model:value="catStore.model.autoReleaseDelay"
        addon-after="s"
        class="w-28"
      />
    </ProListItem>
  </ProList>

  <ProList :title="$t('pages.preference.cat.labels.windowSettings')">
    <ProListItem
      :description="$t('pages.preference.cat.hints.passThrough')"
      :title="$t('pages.preference.cat.labels.passThrough')"
    >
      <Switch v-model:checked="catStore.window.passThrough" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.cat.hints.alwaysOnTop')"
      :title="$t('pages.preference.cat.labels.alwaysOnTop')"
    >
      <Switch v-model:checked="catStore.window.alwaysOnTop" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.cat.hints.hideOnHover')"
      :title="$t('pages.preference.cat.labels.hideOnHover')"
    >
      <Switch v-model:checked="catStore.window.hideOnHover" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.cat.hints.windowSize')"
      :title="$t('pages.preference.cat.labels.windowSize')"
    >
      <InputNumber
        v-model:value="catStore.window.scale"
        addon-after="%"
        class="w-28"
        :min="1"
      />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.cat.labels.windowRadius')">
      <InputNumber
        v-model:value="catStore.window.radius"
        addon-after="%"
        class="w-28"
        :min="0"
      />
    </ProListItem>

    <ProListItem
      :title="$t('pages.preference.cat.labels.opacity')"
      vertical
    >
      <Slider
        v-model:value="catStore.window.opacity"
        class="m-[0]!"
        :max="100"
        :min="10"
        :tip-formatter="(value) => `${value}%`"
      />
    </ProListItem>
  </ProList>

  <ProList :title="$t('pages.preference.cat.labels.statisticsSettings')">
    <ProListItem
      :description="$t('pages.preference.cat.hints.enableStatistics')"
      :title="$t('pages.preference.cat.labels.enableStatistics')"
    >
      <Switch v-model:checked="statisticsStore.settings.enabled" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.cat.hints.mouseClickStatistics')"
      :title="$t('pages.preference.cat.labels.mouseClickStatistics')"
    >
      <Switch v-model:checked="statisticsStore.settings.mouseClickEnabled" />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.cat.labels.resetStatistics')">
      <Popconfirm
        :cancel-text="$t('pages.preference.cat.buttons.cancel')"
        :ok-text="$t('pages.preference.cat.buttons.confirm')"
        :title="$t('pages.preference.cat.hints.resetStatisticsConfirm')"
        @confirm="handleReset"
      >
        <Button danger>
          {{ $t('pages.preference.cat.buttons.reset') }}
        </Button>
      </Popconfirm>
    </ProListItem>
  </ProList>
</template>
