<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { confirm } from '@tauri-apps/plugin-dialog'
import { Space } from 'ant-design-vue'
import { checkInputMonitoringPermission, requestInputMonitoringPermission } from 'tauri-plugin-macos-permissions-api'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { isMac } from '@/utils/platform'

const authorized = ref(false)
const { t } = useI18n()

onMounted(async () => {
  authorized.value = await checkInputMonitoringPermission()

  if (authorized.value) return

  const appWindow = getCurrentWebviewWindow()

  await appWindow.setAlwaysOnTop(true)

  const confirmed = await confirm(t('pages.preference.general.hints.inputMonitoringPermissionGuide'), {
    title: t('pages.preference.general.labels.inputMonitoringPermission'),
    okLabel: t('pages.preference.general.buttons.openNow'),
    cancelLabel: t('pages.preference.general.buttons.openLater'),
    kind: 'warning',
  })

  if (!confirmed) return

  await appWindow.setAlwaysOnTop(false)

  requestInputMonitoringPermission()
})
</script>

<template>
  <ProList
    v-if="isMac"
    :title="$t('pages.preference.general.labels.permissionsSettings')"
  >
    <ProListItem
      :description="$t('pages.preference.general.hints.inputMonitoringPermission')"
      :title="$t('pages.preference.general.labels.inputMonitoringPermission')"
    >
      <Space
        v-if="authorized"
        class="text-success font-bold"
        :size="4"
      >
        <div class="i-solar:verified-check-bold text-4.5" />

        <span class="whitespace-nowrap">{{ $t('pages.preference.general.status.authorized') }}</span>
      </Space>

      <Space
        v-else
        class="cursor-pointer text-danger font-bold"
        :size="4"
        @click="requestInputMonitoringPermission"
      >
        <div class="i-solar:round-arrow-right-bold text-4.5" />

        <span class="whitespace-nowrap">{{ $t('pages.preference.general.status.authorize') }}</span>
      </Space>
    </ProListItem>
  </ProList>
</template>
