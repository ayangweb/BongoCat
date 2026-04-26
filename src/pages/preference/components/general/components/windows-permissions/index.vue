<script setup lang="ts">
import { confirm } from '@tauri-apps/plugin-dialog'
import { exit } from '@tauri-apps/plugin-process'
import { Space } from 'antdv-next'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { isRunningAsAdministrator } from '@/plugins/adminStatus'

const authorized = ref(true)
const { t } = useI18n()

onMounted(async () => {
  authorized.value = await isRunningAsAdministrator()

  if (authorized.value) return

  showAdministratorGuide()
})

async function showAdministratorGuide() {
  const confirmed = await confirm(t('pages.preference.general.hints.administratorPermissionGuide'), {
    title: t('pages.preference.general.labels.administratorPermission'),
    okLabel: t('pages.preference.general.buttons.exitApp'),
    cancelLabel: t('pages.preference.general.buttons.setLater'),
    kind: 'warning',
  })

  if (!confirmed) return

  exit(0)
}
</script>

<template>
  <ProList
    :title="$t('pages.preference.general.labels.permissionsSettings')"
  >
    <ProListItem
      :description="$t('pages.preference.general.hints.administratorPermission')"
      :title="$t('pages.preference.general.labels.administratorPermission')"
    >
      <Space
        v-if="authorized"
        class="text-success font-bold"
        :size="4"
      >
        <div class="i-solar:verified-check-bold text-4.5" />

        <span class="whitespace-nowrap">{{ $t('pages.preference.general.status.adminEnabled') }}</span>
      </Space>

      <Space
        v-else
        class="cursor-pointer text-error font-bold"
        :size="4"
        @click="showAdministratorGuide"
      >
        <div class="i-solar:round-arrow-right-bold text-4.5" />

        <span class="whitespace-nowrap">{{ $t('pages.preference.general.status.viewGuide') }}</span>
      </Space>
    </ProListItem>
  </ProList>
</template>
