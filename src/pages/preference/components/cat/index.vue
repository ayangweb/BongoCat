<script setup lang="ts">
import { Slider, Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()

function scaleFormatter(value?: number) {
  return value === 100 ? '默认' : `${value}%`
}

function opacityFormatter(value?: number) {
  return `${value}%`
}
</script>

<template>
  <ProList title="模型设置">
    <ProListItem
      description="启用后，模型将水平镜像翻转"
      title="镜像模式"
    >
      <Switch v-model:checked="catStore.mirrorMode" />
    </ProListItem>

    <ProListItem
      description="启用后，每只手只显示最后按下的一个按键"
      title="单键模式"
    >
      <Switch v-model:checked="catStore.singleMode" />
    </ProListItem>

    <ProListItem
      description="启用后，鼠标将镜像跟随手部移动"
      title="鼠标镜像"
    >
      <Switch v-model:checked="catStore.mouseMirror" />
    </ProListItem>
  </ProList>

  <ProList title="窗口设置">
    <ProListItem
      description="启用后，窗口不影响对其他应用程序的操作"
      title="窗口穿透"
    >
      <Switch v-model:checked="catStore.penetrable" />
    </ProListItem>

    <ProListItem
      description="启用后，窗口始终显示在其他应用程序上方"
      title="窗口置顶"
    >
      <Switch v-model:checked="catStore.alwaysOnTop" />
    </ProListItem>

    <ProListItem
      description="启用后，拖动窗口到屏幕边缘时会自动吸附"
      title="自动吸附"
    >
      <Switch v-model:checked="catStore.autoSnap" />
    </ProListItem>

    <ProListItem
      description="将鼠标移动到窗口边缘后，也可以拖动调整窗口尺寸"
      title="窗口尺寸"
      vertical
    >
      <Slider
        v-model:value="catStore.scale"
        class="m-0!"
        :max="150"
        :min="50"
        :tip-formatter="scaleFormatter"
      />
    </ProListItem>

    <ProListItem
      title="不透明度"
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
