<script setup lang="ts">
import { Switch } from 'ant-design-vue'

import HotKey from './components/hot-key/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useShortcutEditor } from '@/composables/useShortcutEditor.ts'
import { hideWindow, showWindow } from '@/plugins/window.ts'
import { useCatStore } from '@/stores/cat.ts'
import { useShortcutStore } from '@/stores/shortcut.ts'

const catStore = useCatStore()
const shortcutStore = useShortcutStore()

const { isEditing, pressedKey, onEdit, onSave, hotKey } = useShortcutEditor('visibleCat', () => {
  catStore.visible = !catStore.visible
  catStore.visible ? showWindow('main') : hideWindow('main')
})
</script>

<template>
  <ProList title="全局快捷键">
    <ProListItem title="是否启动">
      <Switch v-model:checked="shortcutStore.enabled" />
    </ProListItem>
  </ProList>

  <ProList title="快捷键列表">
    <ProListItem title="隐藏猫咪">
      <HotKey
        :hot-key="hotKey"
        :is-editing="isEditing"
        :pressed-key="pressedKey"
        @edit="onEdit"
        @save="onSave"
      />
    </ProListItem>
  </ProList>
</template>
