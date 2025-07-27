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
    <!-- 制作模型 Making a model -->
    <FloatButton
      tooltip="Làm một mô hình"
      @click="openUrl('https://juejin.cn/post/7509872655802269731')"
    >
      <template #icon>
        <EditOutlined />
      </template>
    </FloatButton>
    <!-- Transformation model 转换模型 -->
    <FloatButton
      tooltip="Mô hình chuyển đổi"
      @click="openUrl('https://bongocat.vteamer.cc')"
    >
      <template #icon>
        <SyncOutlined />
      </template>
    </FloatButton>
    <!-- 更多模型 More models -->
    <FloatButton
      tooltip="Nhiều mô hình hơn"
      @click="openUrl('https://github.com/ayangweb/Awesome-BongoCat')"
    >
      <template #icon>
        <UnorderedListOutlined />
      </template>
    </FloatButton>
  </FloatButtonGroup>
</template>
