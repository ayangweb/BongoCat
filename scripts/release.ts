import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'

import { name, version } from '../package.json'

const __dirname = dirname(__filename);

(() => {
  const tomlPath = resolve(__dirname, '..', 'src-tauri', 'Cargo.toml')
  const lockPath = resolve(__dirname, '..', 'Cargo.lock')

  for (const path of [tomlPath, lockPath]) {
    let content = readFileSync(path, 'utf-8')

    const regexp = new RegExp(
      `(name\\s*=\\s*"${name}"\\s*version\\s*=\\s*)"(\\d+\\.\\d+\\.\\d+(-\\w+\\.\\d+)?)"`,
    )

    content = content.replace(regexp, `$1"${version}"`)

    writeFileSync(path, content)
  }
})()
