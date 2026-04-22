<script setup lang="ts">
import { Input, InputNumber, Segmented, Switch } from 'ant-design-vue'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import type { AppConfig } from '@/stores/hook'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { useHookStore } from '@/stores/hook'
import { LABEL_SIZES, useLabelStore } from '@/stores/label'

const labelStore = useLabelStore()
const hookStore = useHookStore()

const notificationEnabled = computed(() => hookStore.hookHealth?.notification === 'healthy')
const localConfig = ref<AppConfig | null>(null)
let unlistenPermission: (() => void) | null = null

onMounted(async () => {
  await hookStore.refresh()
  localConfig.value = hookStore.config ? { ...hookStore.config } : null
  const unlisten = await hookStore.setupPermissionListener()
  unlistenPermission = unlisten
})

onUnmounted(() => {
  unlistenPermission?.()
})

async function onNotificationToggle(val: boolean) {
  if (val) {
    await hookStore.installNotificationHook()
  } else {
    await hookStore.uninstallNotificationHook()
  }
}

async function onPretoolToggle(val: boolean) {
  if (val) {
    await hookStore.installPretoolHook()
  } else {
    await hookStore.uninstallPretoolHook()
  }
}

async function onInterceptToggle(val: boolean) {
  await hookStore.toggleIntercept(val)
}

async function saveConfig() {
  if (localConfig.value) {
    await hookStore.saveConfig(localConfig.value)
  }
}
</script>

<template>
  <ProList :title="$t('pages.preference.hook.labels.labelSettings')">
    <ProListItem
      :description="$t('pages.preference.hook.hints.labelText')"
      :title="$t('pages.preference.hook.labels.labelText')"
      vertical
    >
      <Input
        :maxlength="24"
        :placeholder="$t('pages.preference.hook.hints.labelTextPlaceholder')"
        :value="labelStore.defaultText"
        @update:value="value => labelStore.setDefaultText(value ?? '')"
      />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.hook.hints.labelSize')"
      :title="$t('pages.preference.hook.labels.labelSize')"
      vertical
    >
      <Segmented
        v-model:value="labelStore.size"
        block
        :options="LABEL_SIZES.map(value => ({
          label: $t(`pages.preference.hook.options.${value}`),
          value,
        }))"
      />
    </ProListItem>
    <ProListItem
      :description="$t('pages.preference.hook.hints.showSessionTitle')"
      :title="$t('pages.preference.hook.labels.showSessionTitle')"
    >
      <Switch v-model:checked="labelStore.showSessionTitle" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.hook.hints.collapseLongText')"
      :title="$t('pages.preference.hook.labels.collapseLongText')"
    >
      <Switch v-model:checked="labelStore.collapseLongText" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.hook.hints.showDetailedContent')"
      :title="$t('pages.preference.hook.labels.showDetailedContent')"
    >
      <Switch v-model:checked="labelStore.showDetailedContent" />
    </ProListItem>
  </ProList>

  <ProList :title="$t('pages.preference.hook.labels.claudeCodeSettings')">
    <ProListItem
      :description="$t('pages.preference.hook.hints.notificationHook')"
      :title="$t('pages.preference.hook.labels.notificationHook')"
    >
      <Switch
        :checked="notificationEnabled"
        @change="onNotificationToggle"
      />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.hook.hints.pretoolHook')"
      :title="$t('pages.preference.hook.labels.pretoolHook')"
    >
      <Switch
        :checked="hookStore.pretoolHookEnabled"
        @change="onPretoolToggle"
      />
    </ProListItem>

    <ProListItem
      v-if="hookStore.pretoolHookEnabled"
      :description="$t('pages.preference.hook.hints.interceptMode')"
      :title="$t('pages.preference.hook.labels.interceptMode')"
    >
      <Switch
        :checked="hookStore.interceptActive"
        @change="onInterceptToggle"
      />
    </ProListItem>
  </ProList>

  <ProList
    v-if="localConfig"
    :title="$t('pages.preference.hook.labels.advancedConfig')"
  >
    <ProListItem
      :description="$t('pages.preference.hook.hints.hookTimeout')"
      :title="$t('pages.preference.hook.labels.hookTimeout')"
    >
      <InputNumber
        v-model:value="localConfig.hook_timeout_secs"
        :max="300"
        :min="10"
        @blur="saveConfig"
      />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.hook.hints.idleSleep')"
      :title="$t('pages.preference.hook.labels.idleSleep')"
    >
      <InputNumber
        v-model:value="localConfig.idle_sleep_secs"
        :max="3600"
        :min="30"
        @blur="saveConfig"
      />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.hook.hints.activeStateTimeout')"
      :title="$t('pages.preference.hook.labels.activeStateTimeout')"
    >
      <InputNumber
        v-model:value="localConfig.active_state_timeout_secs"
        :max="120"
        :min="5"
        @blur="saveConfig"
      />
    </ProListItem>
  </ProList>
</template>
