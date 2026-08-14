<script setup lang="ts">
import { resolveResource } from '@tauri-apps/api/path'
import { message } from '@tauri-apps/plugin-dialog'
import { openPath } from '@tauri-apps/plugin-opener'
import { Space } from 'antdv-next'
import { checkInputMonitoringPermission, requestInputMonitoringPermission } from 'tauri-plugin-macos-permissions-api'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'

const authorized = ref(false)
const { t } = useI18n()

onMounted(async () => {
  authorized.value = await checkInputMonitoringPermission()

  if (authorized.value) return

  const openSettingsLabel = t('pages.preference.general.buttons.openNow')
  const viewGuideLabel = t('pages.preference.general.status.viewGuide')
  const openLaterLabel = t('pages.preference.general.buttons.openLater')

  const confirmed = await message(t('pages.preference.general.hints.inputMonitoringPermissionGuide'), {
    title: t('pages.preference.general.labels.inputMonitoringPermission'),
    kind: 'warning',
    buttons: {
      yes: openSettingsLabel,
      no: viewGuideLabel,
      cancel: openLaterLabel,
    },
  })

  if (confirmed === openLaterLabel) return

  if (confirmed === viewGuideLabel) {
    const guidePath = await resolveResource('assets/macos-input-monitoring-guide.png')
    await openPath(guidePath)
    return
  }

  requestInputMonitoringPermission()
})
</script>

<template>
  <ProList
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
        class="cursor-pointer text-error font-bold"
        :size="4"
        @click="requestInputMonitoringPermission"
      >
        <div class="i-solar:round-arrow-right-bold text-4.5" />

        <span class="whitespace-nowrap">{{ $t('pages.preference.general.status.authorize') }}</span>
      </Space>
    </ProListItem>
  </ProList>
</template>
