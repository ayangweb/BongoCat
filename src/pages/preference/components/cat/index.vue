<script setup lang="ts">
import { InputNumber, Slider, Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()

function opacityFormatter(value?: number) {
  return `${value}%`
}
</script>

<template>
  <ProList title="Model Settings">
    <ProListItem
      description="When enabled, the model will be flipped horizontally."
      title="Mirror Mode"
    >
      <Switch v-model:checked="catStore.mirrorMode" />
    </ProListItem>

    <ProListItem
      description="When enabled, only the last pressed key will be displayed for each hand."
      title="Single Key Mode"
    >
      <Switch v-model:checked="catStore.singleMode" />
    </ProListItem>

    <ProListItem
      description="When enabled, the mouse will mirror the movement of the hand."
      title="Mouse Mirror"
    >
      <Switch v-model:checked="catStore.mouseMirror" />
    </ProListItem>
  </ProList>

  <ProList title="Window Settings">
    <ProListItem
      description="When enabled, the window will not affect operations on other applications."
      title="Window Penetration"
    >
      <Switch v-model:checked="catStore.penetrable" />
    </ProListItem>

    <ProListItem
      description="When enabled, the window will always be displayed on top of other applications."
      title="Always on Top"
    >
      <Switch v-model:checked="catStore.alwaysOnTop" />
    </ProListItem>

    <ProListItem
      description="After moving the mouse to the edge of the window, you can also drag to adjust the window size."
      title="Window Size"
    >
      <InputNumber
        v-model:value="catStore.scale"
        class="w-28"
        :min="1"
      >
        <template #addonAfter>
          %
        </template>
      </InputNumber>
    </ProListItem>

    <ProListItem
      title="Opacity"
      vertical
    >
      <Slider
        v-model:value="catStore.opacity"
        class="m-0!"
        :max="100"
        :min="10"
        :tip-formatter="opacityFormatter"
      />
    </ProListItem>
  </ProList>
</template>
