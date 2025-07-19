import type { LiteralUnion } from 'ant-design-vue/es/_util/type'

import { invoke } from '@tauri-apps/api/core'
import { watch } from 'vue'

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

export function useGamepad() {
  const { currentModel } = useModelStore()
  const { handlePress, handleRelease, handleAxisChange } = useModel()

  watch(() => currentModel?.mode, (mode) => {
    if (mode === 'gamepad') {
      return invoke(INVOKE_KEY.START_GAMEPAD_LISTING)
    }

    invoke(INVOKE_KEY.STOP_GAMEPAD_LISTING)
  }, { immediate: true })

  useTauriListen<GamepadEvent>(LISTEN_KEY.GAMEPAD_CHANGED, ({ payload }) => {
    const { name, value } = payload

    switch (name) {
      case 'LeftStickX':

        return handleAxisChange('CatParamStickLX', value)
      case 'LeftStickY':

        return handleAxisChange('CatParamStickLY', value)
      case 'RightStickX':

        return handleAxisChange('CatParamStickRX', value)
      case 'RightStickY':

        return handleAxisChange('CatParamStickRY', value)
      case 'LeftThumb':
        return live2d.setParameterValue('CatParamStickLeftDown', value !== 0)
      case 'RightThumb':
        return live2d.setParameterValue('CatParamStickRightDown', value !== 0)
      default:
        return value > 0 ? handlePress(name) : handleRelease(name)
    }
  })
}
