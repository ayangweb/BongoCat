import type { Cubism4InternalModel } from 'pixi-live2d-display'

import { convertFileSrc } from '@tauri-apps/api/core'
import { readDir, readTextFile } from '@tauri-apps/plugin-fs'
import { Cubism4ModelSettings, Live2DModel } from 'pixi-live2d-display'
import { Application, Ticker } from 'pixi.js'

import { join } from './path'

Live2DModel.registerTicker(Ticker)

class Live2d {
  private app: Application | null = null
  public model: Live2DModel | null = null
  private modelWidth = 0
  private modelHeight = 0

  constructor() { }

  private initApp() {
    if (this.app) return

    const view = document.getElementById('live2dCanvas') as HTMLCanvasElement

    this.app = new Application({
      view,
      resizeTo: window,
      backgroundAlpha: 0,
      resolution: devicePixelRatio,
    })
  }

  public async load(path: string) {
    this.initApp()

    this.destroy()

    const files = await readDir(path)

    const modelFile = files.find(file => file.name.endsWith('.model3.json'))

    if (!modelFile) {
      throw new Error('未找到模型主配置文件，请确认模型文件是否完整。')
    }

    const modelPath = join(path, modelFile.name)

    const modelJSON = JSON.parse(await readTextFile(modelPath))

    const modelSettings = new Cubism4ModelSettings({
      ...modelJSON,
      url: convertFileSrc(modelPath),
    })

    modelSettings.replaceFiles((file) => {
      return convertFileSrc(join(path, file))
    })

    this.model = await Live2DModel.from(modelSettings)

    const { width, height } = this.model
    this.modelWidth = width
    this.modelHeight = height

    this.app?.stage.addChild(this.model)

    const { motions, expressions } = modelSettings

    return {
      motions,
      expressions,
    }
  }

  public destroy() {
    this.model?.destroy()
  }

  public fitModel() {
    if (!this.model) return

    const scaleX = innerWidth / this.modelWidth
    const scaleY = innerHeight / this.modelHeight
    const scale = Math.min(scaleX, scaleY)

    this.model.scale.set(scale)
    this.model.x = innerWidth / 2
    this.model.y = innerHeight / 2
    this.model.anchor.set(0.5)
  }

  public playMotion(group: string, index: number) {
    return this.model?.motion(group, index)
  }

  public playExpressions(index: number) {
    return this.model?.expression(index)
  }

  public getCoreModel() {
    const internalModel = this.model?.internalModel as Cubism4InternalModel

    return internalModel?.coreModel
  }

  public getParameterRange(id: string) {
    const coreModel = this.getCoreModel()

    const index = coreModel?.getParameterIndex(id)
    const min = coreModel?.getParameterMinimumValue(index)
    const max = coreModel?.getParameterMaximumValue(index)

    return {
      min,
      max,
    }
  }

  public setParameterValue(id: string, value: number) {
    const coreModel = this.getCoreModel()

    return coreModel?.setParameterValueById?.(id, Number(value))
  }
}

const live2d = new Live2d()

export default live2d
