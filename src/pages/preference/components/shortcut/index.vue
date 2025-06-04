<script setup lang="ts">
import { Switch } from 'ant-design-vue'
import { reactive } from 'vue'

import HotKey from './components/hot-key/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useShortcutEditor } from '@/composables/useShortcutEditor.ts'
import { useShortcutStore } from '@/stores/shortcut.ts'

const shortcutStore = useShortcutStore()

const visibleCat = reactive(useShortcutEditor('visibleCat'))
const mirrorMode = reactive(useShortcutEditor('mirrorMode'))
const penetrable = reactive(useShortcutEditor('penetrable'))
</script>

<template>
  <ProList title="全局快捷键">
    <ProListItem title="是否启动">
      <Switch v-model:checked="shortcutStore.enabled" />
    </ProListItem>
  </ProList>

  <ProList title="快捷键列表">
    <ProListItem title="隐藏猫咪">
      <HotKey v-bind="visibleCat" />
    </ProListItem>
    <ProListItem title="镜像模式">
      <HotKey v-bind="mirrorMode" />
    </ProListItem>
    <ProListItem title="窗口穿透">
      <HotKey v-bind="penetrable" />
    </ProListItem>
  </ProList>
</template>
