<script setup lang="ts">
import { EditOutlined, MenuOutlined, UnorderedListOutlined } from '@ant-design/icons-vue'
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
      tooltip="如何制作模型？"
      @click="openUrl('https://juejin.cn/spost/7509872655802269731')"
    >
      <template #icon>
        <EditOutlined />
      </template>
    </FloatButton>

    <FloatButton
      tooltip="更多模型"
      @click="openUrl('https://github.com/ayangweb/Awesome-BongoCat')"
    >
      <template #icon>
        <UnorderedListOutlined />
      </template>
    </FloatButton>
  </FloatButtonGroup>
</template>
