<script setup lang="ts">
import { getTauriVersion } from '@tauri-apps/api/app'
import { emit } from '@tauri-apps/api/event'
import { appLogDir } from '@tauri-apps/api/path'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { openPath, openUrl } from '@tauri-apps/plugin-opener'
import { arch, platform, version } from '@tauri-apps/plugin-os'
import { Button, message } from 'ant-design-vue'
import { onMounted, ref } from 'vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { GITHUB_LINK, LISTEN_KEY } from '@/constants'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()
const logDir = ref('')

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

  message.success('Đã sao chép thành công') // Copied successfully 复制成功
}

function feedbackIssue() {
  openUrl(`${GITHUB_LINK}/issues/new/choose`)
}
</script>

<template>
  <!--  关于软件  About the software  版本：Version: -->
  <ProList title="Về phần mềm">
    <ProListItem
      :description="`Phiên bản: v${appStore.version}`"
      :title="appStore.name"
    >
      <Button
        type="primary"
        @click="handleUpdate"
      >
        <!--   检查更新   Check for updates   -->
        Kiểm tra cập nhật
      </Button>

      <template #icon>
        <div class="b b-color-2 rounded-xl b-solid">
          <img
            alt=" "
            class="size-12"
            src="/logo.png"
          >
        </div>
      </template>
    </ProListItem>
    <!-- 复制软件信息并提供给 Bug Issue。 Copy the software information and provide it to Bug Issue. -->
    <!-- 软件信息 Software Information -->
    <ProListItem
      description="Sao chép thông tin phần mềm và cung cấp nó cho vấn đề lỗi."
      title="Thông tin phần mềm"
    >
      <!--      复制 copy -->
      <Button @click="copyInfo">
        sao chép
      </Button>
    </ProListItem>
    <!-- 开源地址 Open source address -->
    <ProListItem title="Địa chỉ nguồn mở">
      <Button
        danger
        @click="feedbackIssue"
      >
        <!--  反馈问题     Feedback questions  -->
        Câu hỏi phản hồi
      </Button>

      <template #description>
        <a :href="GITHUB_LINK">
          {{ GITHUB_LINK }}
        </a>
      </template>
    </ProListItem>
    <!-- 软件日志 Software Log -->
    <ProListItem
      :description="logDir"
      title="Nhật ký phần mềm"
    >
      <!--  查看日志   View log -->
      <Button @click="openPath(logDir)">
        Xem nhật ký
      </Button>
    </ProListItem>
  </ProList>
</template>
