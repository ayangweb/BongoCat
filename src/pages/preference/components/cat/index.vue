<script setup lang="ts">
import { InputNumber, Slider, Switch } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useCatStore } from '@/stores/cat'

const catStore = useCatStore()
</script>

<template>
  <!-- 模型设置 Model Settings -->
  <ProList title="Cài đặt mô hình">
    <!--    启用后，模型将水平镜像翻转。 When enabled, the model flips horizontally. -->
    <!-- Mirror mode   镜像模式 -->
    <ProListItem
      description="Khi được bật, mô hình lật theo chiều ngang."
      title="Chế độ gương"
    >
      <Switch v-model:checked="catStore.mirrorMode" />
    </ProListItem>
    <!-- 启用后，每只手只显示最后按下的一个按键。 When enabled, only one last key is displayed on each hand. -->
    <!-- 单键模式   Single-key mode -->
    <ProListItem
      description="Khi được bật, chỉ có một phím cuối cùng được hiển thị trên mỗi tay."
      title="Chế độ đơn lẻ"
    >
      <Switch v-model:checked="catStore.singleMode" />
    </ProListItem>
    <!-- 启用后，鼠标将镜像跟随手部移动。When enabled, the mouse moves the image along with the hand. -->
    <!-- 鼠标镜像 -->
    <ProListItem
      description="Khi được bật, chuột di chuyển hình ảnh cùng với bàn tay."
      title="Gương chuột"
    >
      <Switch v-model:checked="catStore.mouseMirror" />
    </ProListItem>
  </ProList>
  <!-- 窗口设置 Window settings -->
  <ProList title="Cài đặt cửa sổ">
    <!--  启用后，窗口不影响对其他应用程序的操作。  When enabled, the window does not affect operation on other applications. -->
    <!--   窗口穿透 Window penetration -->
    <ProListItem
      description="Khi được bật, cửa sổ không ảnh hưởng đến hoạt động trên các ứng dụng khác."
      title="Thâm nhập cửa sổ"
    >
      <Switch v-model:checked="catStore.penetrable" />
    </ProListItem>
    <!--    启用后，窗口始终显示在其他应用程序上方。When enabled, the window always appears above other applications. -->
    <!-- 窗口置顶Top window -->
    <ProListItem
      description="Khi được bật, cửa sổ luôn xuất hiện trên các ứng dụng khác."
      title="Cửa sổ trên cùng"
    >
      <Switch v-model:checked="catStore.alwaysOnTop" />
    </ProListItem>
    <!--  Move the mouse to the edge of the window, or hold Shift and right-click to resize the window.  将鼠标移至窗口边缘，或按住 Shift 并右键拖动，也可已调整窗口大小。 -->
    <!-- 窗口尺寸 Window size -->
    <ProListItem
      description="Di chuyển chuột đến cạnh của cửa sổ, hoặc giữ Shift và nhấp chuột phải để thay đổi kích thước cửa sổ."
      title="Kích thước cửa sổ"
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
    <!-- Opacity 不透明度 -->
    <ProListItem
      title="Độ mờ"
      vertical
    >
      <Slider
        v-model:value="catStore.opacity"
        class="m-0!"
        :max="100"
        :min="10"
        :tip-formatter="(value) => `${value}%`"
      />
    </ProListItem>
  </ProList>
</template>
