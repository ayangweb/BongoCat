import type { LiteralUnion } from 'ant-design-vue/es/_util/type'

import { sep } from '@tauri-apps/api/path'

export function join(...paths: LiteralUnion<'resources' | 'left-keys' | 'right-keys' | 'background.png' | 'preview.png'>[]) {
  const joinPaths = paths.map((path) => {
    if (path.endsWith(sep())) {
      return path.slice(0, -1)
    }

    return path
  })

  return joinPaths.join(sep())
}
