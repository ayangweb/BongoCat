<script setup lang="ts">
import { EditOutlined, MenuOutlined, SyncOutlined, UnorderedListOutlined } from '@ant-design/icons-vue'
import { openUrl } from '@tauri-apps/plugin-opener'
import { FloatButton, FloatButtonGroup } from 'ant-design-vue'
</script>

<template>
  <FloatButtonGroup
    class="bottom-4 right-4"
    trigger="click"
    type="primary"
  >
    <template #icon>
      <MenuOutlined />
    </template>

    <FloatButton
      :tooltip="$t('pages.preference.model.tooltips.createModel')"
      @click="openUrl('https://juejin.cn/post/7509872655802269731')"
    >
      <template #icon>
        <EditOutlined />
      </template>
    </FloatButton>

    <FloatButton
      :tooltip="$t('pages.preference.model.tooltips.convertModel')"
      @click="openUrl('https://bongocat.vteamer.cc')"
    >
      <template #icon>
        <SyncOutlined />
      </template>
    </FloatButton>

    <FloatButton
      :tooltip="$t('pages.preference.model.tooltips.moreModels')"
      @click="openUrl('https://github.com/ayangweb/Awesome-BongoCat')"
    >
      <template #icon>
        <UnorderedListOutlined />
      </template>
    </FloatButton>
  </FloatButtonGroup>
</template>
