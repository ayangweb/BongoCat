import type { LiteralUnion } from 'ant-design-vue/es/_util/type'

import { invoke } from '@tauri-apps/api/core'
import { reactive, watch } from 'vue'

import { useModel } from './useModel'
import { useTauriListen } from './useTauriListen'

import { INVOKE_KEY, LISTEN_KEY } from '@/constants'
import { useModelStore } from '@/stores/model'
import live2d from '@/utils/live2d'

type GamepadEventName = LiteralUnion<'LeftStickX' | 'LeftStickY' | 'RightStickX' | 'RightStickY' | 'LeftThumb' | 'RightThumb'>

interface GamepadEvent {
  kind: 'ButtonChanged' | 'AxisChanged'
  name: GamepadEventName
  value: number
}

interface StickState {
  x: number
  y: number
  pressed: boolean
}

interface Sticks {
  left: StickState
  right: StickState
}

export function useGamepad() {
  const { currentModel } = useModelStore()
  const { handlePress, handleRelease, handleAxisChange } = useModel()
  const sticks = reactive<Sticks>({
    left: { x: 0, y: 0, pressed: false },
    right: { x: 0, y: 0, pressed: false },
  })

  watch(() => currentModel?.mode, (mode) => {
    if (mode === 'gamepad') {
      return invoke(INVOKE_KEY.START_GAMEPAD_LISTING)
    }

    invoke(INVOKE_KEY.STOP_GAMEPAD_LISTING)
  }, { immediate: true })

  watch(sticks.left, ({ x, y, pressed }) => {
    const moved = x !== 0 || y !== 0

    for (const id of ['CatParamLeftHandDown', 'CatParamStickShowLeftHand']) {
      live2d.setParameterValue(id, moved || pressed)
    }
  }, { deep: true })

  watch(sticks.right, ({ x, y, pressed }) => {
    const moved = x !== 0 || y !== 0

    for (const id of ['CatParamRightHandDown', 'CatParamStickShowRightHand']) {
      live2d.setParameterValue(id, moved || pressed)
    }
  }, { deep: true })

  useTauriListen<GamepadEvent>(LISTEN_KEY.GAMEPAD_CHANGED, ({ payload }) => {
    const { name, value } = payload

    switch (name) {
      case 'LeftStickX':
        sticks.left.x = value

        return handleAxisChange('CatParamStickLX', value)
      case 'LeftStickY':
        sticks.left.y = value

        return handleAxisChange('CatParamStickLY', value)
      case 'RightStickX':
        sticks.right.x = value

        return handleAxisChange('CatParamStickRX', value)
      case 'RightStickY':
        sticks.right.y = value

        return handleAxisChange('CatParamStickRY', value)
      case 'LeftThumb':
        sticks.left.pressed = value !== 0

        return live2d.setParameterValue('CatParamStickLeftDown', value !== 0)
      case 'RightThumb':
        sticks.right.pressed = value !== 0

        return live2d.setParameterValue('CatParamStickRightDown', value !== 0)
      default:
        return value > 0 ? handlePress(name) : handleRelease(name)
    }
  })
}
