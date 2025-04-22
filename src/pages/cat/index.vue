<script setup lang="ts">
import type { SelectProps } from 'ant-design-vue'

import { Select, Slider, Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()

const modeList: SelectProps['options'] = [
  {
    label: '标准模式',
    value: 'standard',
  },
  {
    label: '键盘模式',
    value: 'keyboard',
  },
]
</script>

<template>
  <ProList title="模式设置">
    <ProListItem title="选择模式">
      <Select v-model:value="catStore.mode" :options="modeList" title="选择模式" />
    </ProListItem>
  </ProList>

  <ProList title="窗口设置">
    <ProListItem description="启用后，窗口不影响对其他应用程序的操作" title="窗口穿透">
      <Switch v-model:checked="catStore.penetrable" />
    </ProListItem>

    <ProListItem title="透明度" vertical>
      <Slider v-model:value="catStore.opacity" class="m-0!" />
    </ProListItem>
  </ProList>
</template>
