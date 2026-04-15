import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

export interface CatStore {
  model: {
    mirror: boolean
    mouseMirror: boolean
    motionSound: boolean
    behavior: boolean
    autoReleaseDelay: number
    /**
     * EMA smoothing factor for global mouse tracking (0–1).
     * Lower values produce smoother movement at the cost of a slight lag;
     * higher values are more responsive but may show micro-jitter on
     * high-report-rate (>500 Hz) mice.
     * Recommended range: 0.1 – 0.5.  Default: 0.25.
     */
    mouseSmoothingAlpha: number
  }
  window: {
    visible: boolean
    passThrough: boolean
    alwaysOnTop: boolean
    scale: number
    opacity: number
    radius: number
    hideOnHover: boolean
    keepInScreen: boolean
  }
}

export const useCatStore = defineStore('cat', () => {
  /* ------------ 废弃字段（后续删除） ------------ */

  /** @deprecated 请使用 `model.mirror` */
  const mirrorMode = ref(false)

  /** @deprecated 请使用 `model.mouseMirror` */
  const mouseMirror = ref(false)

  /** @deprecated 请使用 `window.passThrough` */
  const penetrable = ref(false)

  /** @deprecated 请使用 `window.alwaysOnTop` */
  const alwaysOnTop = ref(true)

  /** @deprecated 请使用 `window.scale` */
  const scale = ref(100)

  /** @deprecated 请使用 `window.opacity` */
  const opacity = ref(100)

  /** @deprecated 用于标识数据是否已迁移，后续版本将删除 */
  const migrated = ref(false)

  const model = reactive<CatStore['model']>({
    mirror: false,
    mouseMirror: false,
    motionSound: true,
    behavior: true,
    autoReleaseDelay: 3,
    mouseSmoothingAlpha: 0.25,
  })

  const window = reactive<CatStore['window']>({
    visible: true,
    passThrough: false,
    alwaysOnTop: false,
    scale: 100,
    opacity: 100,
    radius: 0,
    hideOnHover: false,
    keepInScreen: true,
  })

  const init = () => {
    if (migrated.value) return

    model.mirror = mirrorMode.value
    model.mouseMirror = mouseMirror.value

    window.visible = true
    window.passThrough = penetrable.value
    window.alwaysOnTop = alwaysOnTop.value
    window.scale = scale.value
    window.opacity = opacity.value

    migrated.value = true
  }

  return {
    migrated,
    model,
    window,
    init,
  }
})
