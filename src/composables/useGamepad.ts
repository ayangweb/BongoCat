// import { invoke } from '@tauri-apps/api/core'
// import { ref, watch } from 'vue'

// import { useTauriListen } from './useTauriListen'

// import { INVOKE_KEY, LISTEN_KEY } from '@/constants'
// import { useModelStore } from '@/stores/model'

// interface GamepadEvent {
//   kind: 'ButtonChanged' | 'AxisChanged'
//   name: string
//   value: number
// }

export function useGamepad() {
  // const { currentModel } = useModelStore()
  // const pressedGamepadKeys = ref<string[]>([])

  // watch(() => currentModel?.mode, (value) => {
  //   if (value === 'gamepad') {
  //     return invoke(INVOKE_KEY.START_GAMEPAD_LISTING)
  //   }

  //   invoke(INVOKE_KEY.STOP_GAMEPAD_LISTING)
  // }, { immediate: true })

  // useTauriListen<GamepadEvent>(LISTEN_KEY.GAMEPAD_CHANGED, ({ payload }) => {
  //   // console.log('payload', payload)

  //   // const { kind, name, value } = payload
  // })
}
