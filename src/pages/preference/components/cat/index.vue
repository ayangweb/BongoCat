<script setup lang="ts">
import { InputNumber, Slider, Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()
</script>

<template>
  <ProList title="模型设置">
    <ProListItem
      description="启用后，模型将水平镜像翻转。"
      title="镜像模式"
    >
      <Switch v-model:checked="catStore.model.mirror" />
    </ProListItem>

    <ProListItem
      description="启用后，每只手只显示最后按下的一个按键。"
      title="单键模式"
    >
      <Switch v-model:checked="catStore.model.single" />
    </ProListItem>

    <ProListItem
      description="启用后，鼠标将镜像跟随手部移动。"
      title="鼠标镜像"
    >
      <Switch v-model:checked="catStore.model.mouseMirror" />
    </ProListItem>
  </ProList>

  <ProList title="窗口设置">
    <ProListItem
      description="启用后，窗口不影响对其他应用程序的操作。"
      title="窗口穿透"
    >
      <Switch v-model:checked="catStore.window.passThrough" />
    </ProListItem>

    <ProListItem
      description="启用后，窗口始终显示在其他应用程序上方。"
      title="窗口置顶"
    >
      <Switch v-model:checked="catStore.window.alwaysOnTop" />
    </ProListItem>

    <ProListItem
      description="将鼠标移至窗口边缘，或按住 Shift 并右键拖动，也可以调整窗口大小。"
      title="窗口尺寸"
    >
      <InputNumber
        v-model:value="catStore.window.scale"
        addon-after="%"
        class="w-28"
        :min="1"
      />
    </ProListItem>

    <ProListItem title="窗口圆角">
      <InputNumber
        v-model:value="catStore.window.radius"
        addon-after="%"
        class="w-28"
        :min="0"
      />
    </ProListItem>

    <ProListItem
      title="不透明度"
      vertical
    >
      <Slider
        v-model:value="catStore.window.opacity"
        class="m-0!"
        :max="100"
        :min="10"
        :tip-formatter="(value) => `${value}%`"
      />
    </ProListItem>
  </ProList>
</template>
