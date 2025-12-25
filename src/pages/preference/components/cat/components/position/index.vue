<script setup lang="ts">
import { Select, SelectOption } from 'ant-design-vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()
</script>

<template>
  <ProListItem
    description="启动应用、更改尺寸、更换模型、更改电脑分辨率后生效"
    title="窗口位置"
  >
    <Select v-model:value="catStore.window.position">
      <SelectOption value="bottomRight">
        右下角
      </SelectOption>
      <SelectOption value="bottomLeft">
        左下角
      </SelectOption>
      <SelectOption value="topLeft">
        左上角
      </SelectOption>
      <SelectOption value="topRight">
        右上角
      </SelectOption>
    </Select>
  </ProListItem>
</template>
