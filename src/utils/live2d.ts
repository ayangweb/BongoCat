import type { MotionInfo } from 'easy-live2d'

import { convertFileSrc } from '@tauri-apps/api/core'
import { readDir, readTextFile } from '@tauri-apps/plugin-fs'
import { Config, CubismSetting, Live2DSprite, Priority } from 'easy-live2d'
import { groupBy } from 'es-toolkit/compat'
import JSON5 from 'json5'
import { Application, Ticker } from 'pixi.js'

import type { ModelSize } from '@/composables/useModel'

import { i18n } from '@/locales'

import { join } from './path'

Config.MouseFollow = false

class Live2d {
  private app: Application | null = null
  private appInitPromise: Promise<void> | null = null
  private loadGeneration = 0
  private loadQueue: Promise<void> = Promise.resolve()
  public model: Live2DSprite | null = null

  constructor() { }

  private async initApp() {
    if (this.appInitPromise) {
      await this.appInitPromise

      return
    }

    if (this.app) return

    const view = document.getElementById('live2dCanvas') as HTMLCanvasElement
    const app = new Application()

    this.app = app
    this.appInitPromise = app.init({
      view,
      resizeTo: window,
      backgroundAlpha: 0,
      autoDensity: true,
      resolution: devicePixelRatio,
    })

    try {
      await this.appInitPromise
    } catch (error) {
      if (this.app === app) this.app = null

      throw error
    } finally {
      this.appInitPromise = null
    }
  }

  public async load(path: string) {
    const generation = ++this.loadGeneration
    const previousLoad = this.loadQueue
    let releaseLoad!: () => void

    this.loadQueue = new Promise((resolve) => {
      releaseLoad = resolve
    })

    try {
      await previousLoad

      this.assertGeneration(generation)
      this.destroyModel()

      await this.initApp()

      this.assertGeneration(generation)

      const files = await readDir(path)

      this.assertGeneration(generation)

      const modelFile = files.find(file => file.name.endsWith('.model3.json'))

      if (!modelFile) {
        throw new Error(i18n.global.t('utils.live2d.hints.notFound'))
      }

      const modelPath = join(path, modelFile.name)
      const modelText = await readTextFile(modelPath)

      this.assertGeneration(generation)

      const modelJSON = JSON5.parse(modelText)

      const modelSetting = new CubismSetting({
        modelJSON,
      })

      modelSetting.redirectPath(({ file }) => {
        return convertFileSrc(join(path, file))
      })

      const model = new Live2DSprite({
        modelSetting,
        ticker: Ticker.shared,
      })

      this.app?.stage.addChild(model)

      try {
        await model.ready

        this.assertGeneration(generation)

        this.model = model

        const { width, height } = model
        const motions = groupBy(model.getMotions(), 'group')
        const expressions = model.getExpressions()

        return {
          width,
          height,
          motions,
          expressions,
        }
      } catch (error) {
        if (this.model === model) this.model = null

        model.destroy()

        throw error
      }
    } finally {
      releaseLoad()
    }
  }

  public destroy() {
    ++this.loadGeneration
    this.destroyModel()
  }

  private assertGeneration(generation: number) {
    if (generation === this.loadGeneration) return

    throw new DOMException('Live2D model load was superseded', 'AbortError')
  }

  private destroyModel() {
    if (!this.model) return

    this.model.destroy()

    this.model = null
  }

  public resizeModel(modelSize: ModelSize) {
    if (!this.model) return

    const { width, height } = modelSize

    const scaleX = innerWidth / width
    const scaleY = innerHeight / height
    const scale = Math.min(scaleX, scaleY)

    this.model.scale.set(scale)
    this.model.x = innerWidth / 2
    this.model.y = innerHeight / 2
    this.model.anchor.set(0.5)
  }

  public startMotion(motion: MotionInfo) {
    return this.model?.startMotion({
      ...motion,
      priority: Priority.Normal,
    })
  }

  public setExpression(index: number) {
    return this.model?.setExpression({ index })
  }

  public getParameterValueRange(id: string) {
    return this.model?.getParameterValueRangeById(id)
  }

  public setParameterValue(id: string, value: number | boolean) {
    return this.model?.setParameterValueById(id, Number(value))
  }

  public setMotionSoundEnabled(enabled: boolean) {
    Config.MotionSound = enabled
  }

  public setMaxFPS(fps: number) {
    Ticker.shared.maxFPS = fps
  }
}

const live2d = new Live2d()

export default live2d
