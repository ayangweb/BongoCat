import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface HookHealth {
  notification: 'healthy' | 'inactive' | 'broken'
  preToolUse: 'healthy' | 'inactive' | 'broken'
  settingsValid: boolean
}

export interface AppConfig {
  idle_sleep_secs: number
  session_window_secs: number
  hook_timeout_secs: number
  jsonl_debounce_ms: number
  active_state_timeout_secs: number
  session_poll_fallback_secs: number
  cursor_track_near_ms: number
  cursor_track_far_ms: number
}

export interface PermissionRequest {
  requestId: string
  sessionId: string
  toolName: string
  toolInput: string | null
  timestamp: number
}

export const useHookStore = defineStore('hook', () => {
  const hookHealth = ref<HookHealth | null>(null)
  const pretoolHookEnabled = ref(false)
  const interceptActive = ref(false)
  const config = ref<AppConfig | null>(null)
  const pendingRequests = ref<PermissionRequest[]>([])

  async function refresh() {
    const [health, pretool, intercept, cfg] = await Promise.all([
      invoke<HookHealth>('check_hook_status'),
      invoke<boolean>('check_pretooluse_hook_status'),
      invoke<boolean>('get_intercept_active'),
      invoke<AppConfig>('get_config'),
    ])
    hookHealth.value = health
    pretoolHookEnabled.value = pretool
    interceptActive.value = intercept
    config.value = cfg
  }

  async function installNotificationHook() {
    await invoke('install_notification_hook')
    await refresh()
  }

  async function uninstallNotificationHook() {
    await invoke('uninstall_notification_hook')
    await refresh()
  }

  async function installPretoolHook() {
    await invoke('install_pretooluse_hook')
    await refresh()
  }

  async function uninstallPretoolHook() {
    await invoke('uninstall_pretooluse_hook')
    await refresh()
  }

  async function toggleIntercept(active: boolean) {
    await invoke('set_intercept_active', { active })
    interceptActive.value = active
  }

  async function saveConfig(newConfig: AppConfig) {
    await invoke('update_config', { newConfig })
    config.value = { ...newConfig }
  }

  async function respondPermission(requestId: string, decision: 'allow' | 'deny') {
    await invoke('respond_permission', { requestId, decision })
    pendingRequests.value = pendingRequests.value.filter(r => r.requestId !== requestId)
  }

  function setupPermissionListener() {
    return listen<PermissionRequest>('permission-request', (event) => {
      const req = event.payload
      if (!pendingRequests.value.find(r => r.requestId === req.requestId)) {
        pendingRequests.value = [...pendingRequests.value, req]
      }
    })
  }

  return {
    hookHealth,
    pretoolHookEnabled,
    interceptActive,
    config,
    pendingRequests,
    refresh,
    installNotificationHook,
    uninstallNotificationHook,
    installPretoolHook,
    uninstallPretoolHook,
    toggleIntercept,
    saveConfig,
    respondPermission,
    setupPermissionListener,
  }
})
