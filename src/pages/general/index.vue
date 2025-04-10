<script setup lang="ts">
import { Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
</script>

<template>
  <ProList title="更新设置">
    <ProListItem title="自动检查更新">
      <Switch />
    </ProListItem>
  </ProList>
</template>
