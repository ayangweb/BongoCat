import { theme } from 'ant-design-vue'
import { kebabCase } from 'es-toolkit'

export function useThemeVars() {
  const { defaultAlgorithm, darkAlgorithm, defaultConfig } = theme

  const generateColorVars = () => {
    const { token } = defaultConfig

    const colors = [
      defaultAlgorithm(token),
      darkAlgorithm(token),
    ]

    for (const [index, item] of colors.entries()) {
      const isDark = index !== 0
      const vars: Record<string, any> = {}

      for (const [key, value] of Object.entries(item)) {
        vars[`--ant-${kebabCase(key)}`] = value
      }

      const style = document.createElement('style')
      style.dataset.theme = isDark ? 'dark' : 'light'
      const selector = isDark ? 'html.dark' : ':root'
      const values = Object.entries(vars).map(([key, value]) => `${key}: ${value};`)

      style.innerHTML = `${selector}{\n${values.join('\n')}\n}`
      document.head.appendChild(style)
    }
  }

  return {
    generateColorVars,
  }
}
