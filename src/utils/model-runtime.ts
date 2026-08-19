import type { MotionInfo } from 'easy-live2d'

import type { ModelRenderer } from '@/stores/model'

import live2d from './live2d'
import sprite from './sprite'

class ModelRuntime {
  private renderer: ModelRenderer = 'live2d'
  private mirrored = false
  private loadGeneration = 0

  public async load(path: string, renderer: ModelRenderer) {
    const generation = ++this.loadGeneration

    this.destroyRenderers()

    this.renderer = renderer

    const result = renderer === 'live2d'
      ? await live2d.load(path)
      : await sprite.load(path)

    if (generation !== this.loadGeneration) {
      throw new DOMException('Model load was superseded', 'AbortError')
    }

    if (renderer === 'sprite') sprite.setMirrored(this.mirrored)

    return result
  }

  public destroy() {
    ++this.loadGeneration
    this.destroyRenderers()
  }

  public resizeModel(size: { width: number, height: number }) {
    if (this.renderer === 'sprite') {
      sprite.resizeModel(size)
    } else {
      live2d.resizeModel(size)
    }
  }

  public startMotion(motion: MotionInfo) {
    if (this.renderer !== 'live2d') return

    return live2d.startMotion(motion)
  }

  public setExpression(index: number) {
    if (this.renderer !== 'live2d') return

    return live2d.setExpression(index)
  }

  public getParameterValueRange(id: string) {
    if (this.renderer !== 'live2d') return

    return live2d.getParameterValueRange(id)
  }

  public setParameterValue(id: string, value: number | boolean) {
    if (this.renderer !== 'live2d') return

    return live2d.setParameterValue(id, value)
  }

  public handleKeyboard(key: string, pressed: boolean, label?: string | null) {
    if (this.renderer !== 'sprite') return

    return sprite.handleKeyboard(key, pressed, label ?? void 0)
  }

  public handleMouse(button: string, pressed: boolean) {
    if (this.renderer !== 'sprite') return

    return sprite.handleMouse(button, pressed)
  }

  public readonly setMotionSoundEnabled = (enabled: boolean) => {
    live2d.setMotionSoundEnabled(enabled)
  }

  public readonly setMaxFPS = (fps: number) => {
    live2d.setMaxFPS(fps)
    sprite.setMaxFPS(fps)
  }

  public readonly setMirrored = (mirrored: boolean) => {
    this.mirrored = mirrored
    sprite.setMirrored(mirrored)
  }

  private destroyRenderers() {
    live2d.destroy()
    sprite.destroy()
  }
}

const modelRuntime = new ModelRuntime()

export default modelRuntime
