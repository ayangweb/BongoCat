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

  constructor() { }

  private mount() {
    const view = document.getElementById('live2dCanvas') as HTMLCanvasElement

    this.app = new Application({
      view,
      resizeTo: window,
      backgroundAlpha: 0,
      autoDensity: true,
      resolution: devicePixelRatio,
    })
  }

  public async load(path: string) {
    if (!this.app) {
      this.mount()
    }

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

  public playMotion(group: string, index: number) {
    return this.model?.motion(group, index)
  }

  public playExpressions(index: number) {
    return this.model?.expression(index)
  }

  public setParameterValue(id: string, value: number | boolean) {
    const internalModel = this.model?.internalModel as Cubism4InternalModel

    return internalModel?.coreModel?.setParameterValueById?.(id, Number(value))
  }
}

const live2d = new Live2d()

export default live2d
