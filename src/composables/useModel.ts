import { Live2DModel } from 'pixi-live2d-display'
import { Application } from 'pixi.js'
import { onMounted, ref } from 'vue'

interface Motion {
  Name: string
  File: string
  Sound?: string
  FadeInTime: number
  FadeOutTime: number
}

interface Expression {
  Name: string
  File: string
}

export function useModel() {
  const mode = ref('standard')
  const model = ref<Live2DModel>()
  const motions = ref<Record<string, Motion[]>>({})
  const expressions = ref<Expression[]>([])

  onMounted(() => {
    loadModel()
  })

  const loadModel = async () => {
    destroyModel()

    const view = document.getElementById('live2dCanvas') as HTMLCanvasElement

    const app = new Application({
      view,
      resizeTo: window,
      backgroundAlpha: 0,
    })

    const loadedModel = await Live2DModel.from(`/models/${mode.value}/cat.model3.json`)

    app.stage.addChild(loadedModel)

    const { definitions, expressionManager }
      = loadedModel.internalModel.motionManager

    model.value = loadedModel
    motions.value = definitions as Record<string, Motion[]>
    expressions.value = expressionManager?.definitions as Expression[]
  }

  const destroyModel = () => {
    model.value?.destroy()
  }

  const setParameterValue = (id: string, value: number | boolean) => {
    return model.value?.internalModel.coreModel.setParameterValueById(id, Number(value))
  }

  return {
    model,
    motions,
    expressions,
    loadModel,
    destroyModel,
    setParameterValue,
  }
}
