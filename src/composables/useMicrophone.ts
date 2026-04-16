import { message } from 'ant-design-vue'
import { onUnmounted, ref, watch } from 'vue'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import live2d from '@/utils/live2d'

export function useMicrophone() {
  const catStore = useCatStore()
  const modelStore = useModelStore()

  const audioContext = ref<AudioContext | null>(null)
  const analyser = ref<AnalyserNode | null>(null)
  const microphone = ref<MediaStreamAudioSourceNode | null>(null)
  const mediaStream = ref<MediaStream | null>(null)
  const animationFrameId = ref<number | null>(null)

  const volumeLevel = ref(0) // 当前音量级别 (0-100)
  const frequencyData = ref<Uint8Array>(new Uint8Array(0))
  const timeDomainData = ref<Uint8Array>(new Uint8Array(0))

  // 错误处理映射
  const errorMessages: Record<string, string> = {
    NotAllowedError: '麦克风权限被拒绝，请在系统设置中启用',
    NotFoundError: '未找到可用的麦克风设备',
    NotReadableError: '麦克风设备被其他应用占用',
    OverconstrainedError: '无法满足音频约束条件',
    SecurityError: '安全限制阻止访问麦克风',
    AbortError: '音频设备访问被中止',
  }

  // 初始化音频上下文和麦克风
  async function startMicrophone() {
    try {
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        throw new Error('当前环境不支持麦克风访问')
      }

      // 请求麦克风权限
      mediaStream.value = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
          sampleRate: 44100,
        },
      })

      // 创建音频上下文
      audioContext.value = new (window.AudioContext || (window as any).webkitAudioContext)()
      analyser.value = audioContext.value.createAnalyser()

      // 配置分析器
      analyser.value.fftSize = 2048
      analyser.value.smoothingTimeConstant = catStore.model.microphoneSmoothing / 100

      microphone.value = audioContext.value.createMediaStreamSource(mediaStream.value)
      microphone.value.connect(analyser.value)

      frequencyData.value = new Uint8Array(analyser.value.frequencyBinCount)
      timeDomainData.value = new Uint8Array(analyser.value.fftSize)

      // 确保音频上下文处于运行状态
      if (audioContext.value.state !== 'running') {
        await audioContext.value.resume()
      }

      // 开始音频分析循环
      startAudioAnalysis()
    } catch (error: any) {
      const errorMessage = errorMessages[error.name] || `麦克风访问失败: ${error.message}`
      message.error(errorMessage)
      stopMicrophone()
      // 自动禁用麦克风功能
      catStore.model.microphoneEnabled = false
    }
  }

  function stopMicrophone() {
    if (animationFrameId.value) {
      cancelAnimationFrame(animationFrameId.value)
      animationFrameId.value = null
    }

    if (mediaStream.value) {
      mediaStream.value.getTracks().forEach(track => track.stop())
      mediaStream.value = null
    }

    if (audioContext.value && audioContext.value.state !== 'closed') {
      audioContext.value.close()
    }

    audioContext.value = null
    analyser.value = null
    microphone.value = null
    volumeLevel.value = 0

    // 重置 Live2D 嘴部参数
    resetMouthParameters()
  }

  function startAudioAnalysis() {
    if (!analyser.value || !audioContext.value) return

    const analyseAudio = () => {
      if (!analyser.value || !audioContext.value) {
        return
      }

      // 更新时间域数据（用于音量计算）
      analyser.value.getByteTimeDomainData(timeDomainData.value)

      // 计算音量（RMS）
      let sum = 0
      for (let i = 0; i < timeDomainData.value.length; i++) {
        const value = (timeDomainData.value[i] - 128) / 128
        sum += value * value
      }
      const rms = Math.sqrt(sum / timeDomainData.value.length)

      // 应用灵敏度调整
      const sensitivity = catStore.model.microphoneSensitivity / 10
      let adjustedVolume = Math.min(rms * sensitivity * 3, 1) // 3倍增益
      adjustedVolume = adjustedVolume ** 0.7 // 非线性映射，更自然

      // 转换为百分比 (0-100)
      volumeLevel.value = adjustedVolume * 100

      // 应用阈值过滤
      const threshold = catStore.model.microphoneThreshold / 100
      const effectiveVolume = volumeLevel.value / 100 >= threshold ? volumeLevel.value : 0

      // 更新Live2D参数
      updateLive2DParameters(effectiveVolume)

      // 继续循环（限制在~60fps）
      animationFrameId.value = requestAnimationFrame(analyseAudio)
    }

    animationFrameId.value = requestAnimationFrame(analyseAudio)
  }

  function resetMouthParameters() {
    const mouthOpenRange = live2d.getParameterValueRange('ParamMouthOpenY')
    if (mouthOpenRange) {
      live2d.setParameterValue('ParamMouthOpenY', mouthOpenRange.min)
    }
    const mouthFormRange = live2d.getParameterValueRange('ParamMouthForm')
    if (mouthFormRange) {
      live2d.setParameterValue('ParamMouthForm', (mouthFormRange.min + mouthFormRange.max) / 2)
    }
  }

  function updateLive2DParameters(volume: number) {
    if (!modelStore.currentModel) return

    // 映射到嘴部开合参数 ParamMouthOpenY
    const mouthOpenRange = live2d.getParameterValueRange('ParamMouthOpenY')
    if (mouthOpenRange) {
      const { min, max } = mouthOpenRange
      // volume是0-100，映射到参数范围
      const mouthOpenValue = min + (volume / 100) * (max - min)
      live2d.setParameterValue('ParamMouthOpenY', mouthOpenValue)
    }

    // 映射到嘴型参数 ParamMouthForm（基于频率特征）
    if (analyser.value && audioContext.value) {
      // 获取频率数据
      analyser.value.getByteFrequencyData(frequencyData.value)

      // 寻找主导频率（85-255Hz人声范围）
      const sampleRate = audioContext.value.sampleRate
      const frequencyResolution = sampleRate / analyser.value.fftSize
      const minIndex = Math.floor(85 / frequencyResolution)
      const maxIndex = Math.floor(255 / frequencyResolution)

      let maxValue = 0
      let dominantIndex = minIndex

      for (let i = minIndex; i < maxIndex; i++) {
        if (frequencyData.value[i] > maxValue) {
          maxValue = frequencyData.value[i]
          dominantIndex = i
        }
      }

      const dominantFrequency = dominantIndex * frequencyResolution

      // 将频率映射到嘴型参数 (85-255Hz 映射到 0-1)
      const normalizedFreq = Math.max(0, Math.min(1, (dominantFrequency - 85) / (255 - 85)))

      const mouthFormRange = live2d.getParameterValueRange('ParamMouthForm')
      if (mouthFormRange) {
        const { min, max } = mouthFormRange
        const mouthFormValue = min + normalizedFreq * (max - min)
        live2d.setParameterValue('ParamMouthForm', mouthFormValue)
      }
    }
  }

  // 监听配置变化
  watch(() => catStore.model.microphoneEnabled, (enabled) => {
    if (enabled) {
      startMicrophone()
    } else {
      stopMicrophone()
    }
  }, { immediate: true })

  // 监听平滑度变化
  watch(() => catStore.model.microphoneSmoothing, (smoothing) => {
    if (analyser.value) {
      analyser.value.smoothingTimeConstant = smoothing / 100
    }
  }, { immediate: true })

  // 清理
  onUnmounted(() => {
    stopMicrophone()
  })

  return {
    startMicrophone,
    stopMicrophone,
    volumeLevel,
    isActive: () => !!mediaStream.value,
  }
}
