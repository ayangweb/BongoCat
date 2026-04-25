import { invoke } from '@tauri-apps/api/core'

const COMMAND = {
  IS_RUNNING_AS_ADMINISTRATOR: 'plugin:admin-status|is_running_as_administrator',
}

export function isRunningAsAdministrator() {
  return invoke<boolean>(COMMAND.IS_RUNNING_AS_ADMINISTRATOR)
}
