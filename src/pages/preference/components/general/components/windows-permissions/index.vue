<script setup lang="ts">
import { Modal, Space } from 'antdv-next'
import { h, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { isRunningAsAdministrator } from '@/plugins/admin'
import { isWindows } from '@/utils/platform'

const authorized = ref(true)
const { t } = useI18n()

onMounted(async () => {
  if (!isWindows) return

  authorized.value = await isRunningAsAdministrator()

  if (authorized.value) return

  showAdministratorGuide()
})

function showAdministratorGuide() {
  Modal.warning({
    centered: true,
    title: t('pages.preference.general.labels.administratorPermission'),
    content: h(
      'div',
      { style: 'white-space: pre-line;' },
      t('pages.preference.general.hints.administratorPermissionGuide'),
    ),
  })
}
</script>

<template>
  <ProList
    v-if="isWindows"
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

        <span class="whitespace-nowrap">{{ $t('pages.preference.general.status.authorized') }}</span>
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
