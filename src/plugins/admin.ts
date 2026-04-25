import { invoke } from '@tauri-apps/api/core'

const COMMAND = {
  IS_ELEVATED: 'plugin:admin-status|is_elevated',
}

export function isRunningAsAdministrator() {
  return invoke<boolean>(COMMAND.IS_ELEVATED)
}
