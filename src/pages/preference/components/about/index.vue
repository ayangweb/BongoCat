<script setup lang="ts">
import { getTauriVersion } from '@tauri-apps/api/app'
import { emit } from '@tauri-apps/api/event'
import { appLogDir } from '@tauri-apps/api/path'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { openPath, openUrl } from '@tauri-apps/plugin-opener'
import { arch, platform, version } from '@tauri-apps/plugin-os'
import { Button, message } from 'ant-design-vue'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { GITHUB_LINK, LISTEN_KEY } from '@/constants'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()
const logDir = ref('')
const { t } = useI18n()

onMounted(async () => {
  logDir.value = await appLogDir()
})

function handleUpdate() {
  emit(LISTEN_KEY.UPDATE_APP)
}

async function copyInfo() {
  const info = {
    appName: appStore.name,
    appVersion: appStore.version,
    tauriVersion: await getTauriVersion(),
    platform: platform(),
    platformArch: arch(),
    platformVersion: version(),
  }

  await writeText(JSON.stringify(info, null, 2))

  message.success(t('pages.preference.about.hints.copySuccess'))
}

function feedbackIssue() {
  openUrl(`${GITHUB_LINK}/issues/new/choose`)
}
</script>

<template>
  <ProList :title="$t('pages.preference.about.labels.aboutApp')">
    <ProListItem
      :description="`v${appStore.version}`"
      :title="appStore.name"
    >
      <Button
        type="primary"
        @click="handleUpdate"
      >
        {{ $t('pages.preference.about.buttons.checkUpdate') }}
      </Button>

      <template #icon>
        <div class="b b-color-2 rounded-xl b-solid">
          <img
            class="size-12"
            src="/logo.png"
          >
        </div>
      </template>
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.about.hints.appInfo')"
      :title="$t('pages.preference.about.labels.appInfo')"
    >
      <Button @click="copyInfo">
        {{ $t('pages.preference.about.buttons.copy') }}
      </Button>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.about.labels.openSource')">
      <Button
        danger
        @click="feedbackIssue"
      >
        {{ $t('pages.preference.about.buttons.feedbackIssues') }}
      </Button>

      <template #description>
        <a :href="GITHUB_LINK">
          {{ GITHUB_LINK }}
        </a>
      </template>
    </ProListItem>

    <ProListItem
      :description="logDir"
      :title="$t('pages.preference.about.labels.appLog')"
    >
      <Button @click="openPath(logDir)">
        {{ $t('pages.preference.about.buttons.viewLog') }}
      </Button>
    </ProListItem>
  </ProList>
</template>
