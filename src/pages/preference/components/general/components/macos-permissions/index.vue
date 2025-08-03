<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { message } from '@tauri-apps/plugin-dialog'
import { Space } from 'ant-design-vue'
import { checkInputMonitoringPermission, requestInputMonitoringPermission } from 'tauri-plugin-macos-permissions-api'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { isMac } from '@/utils/platform'

const { t } = useI18n()
const authorized = ref(false)

onMounted(async () => {
  authorized.value = await checkInputMonitoringPermission()

  if (authorized.value) return

  const appWindow = getCurrentWebviewWindow()

  await appWindow.setAlwaysOnTop(true)

  await message(t('permissions.input.modal.content'), {
    title: t('permissions.input.modal.title'),
    okLabel: t('permissions.input.modal.ok'),
    kind: 'warning',
  })

  await appWindow.setAlwaysOnTop(false)

  requestInputMonitoringPermission()
})
</script>

<template>
  <ProList
    v-if="isMac"
    :title="t('permissions.title')"
  >
    <ProListItem
      :description="t('permissions.input.desc')"
      :title="t('permissions.input.title')"
    >
      <Space
        v-if="authorized"
        class="text-success font-bold"
        :size="4"
      >
        <div class="i-solar:verified-check-bold text-4.5" />

        <span>{{ t('permissions.input.authorized') }}</span>
      </Space>

      <Space
        v-else
        class="cursor-pointer text-danger font-bold"
        :size="4"
        @click="requestInputMonitoringPermission"
      >
        <div class="i-solar:round-arrow-right-bold text-4.5" />

        <span>{{ t('permissions.input.unauthorized') }}</span>
      </Space>
    </ProListItem>
  </ProList>
</template>
