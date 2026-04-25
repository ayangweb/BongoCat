import { invoke } from '@tauri-apps/api/core'

const COMMAND = {
  IS_ELEVATED: 'plugin:admin-status|is_elevated',
  RELAUNCH_AS_ADMINISTRATOR: 'plugin:admin-status|relaunch_as_administrator',
}

export function isRunningAsAdministrator() {
  return invoke<boolean>(COMMAND.IS_ELEVATED)
}

export function relaunchAsAdministrator() {
  return invoke<void>(COMMAND.RELAUNCH_AS_ADMINISTRATOR)
}
