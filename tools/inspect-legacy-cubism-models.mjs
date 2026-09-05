#!/usr/bin/env node

import { Buffer } from 'node:buffer'
import crypto from 'node:crypto'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.dirname(SCRIPT_DIR)
const SNAPSHOT_PATH = path.join(
  ROOT,
  'shared/fixtures/model-fixtures/legacy-core-baseline.json',
)
const CORE_PATH = 'public/js/live2dcubismcore.min.js'
const CORE_SHA256
  = '25ae938cb4fe282ce189b357bcc97e603d1e1f7ec78bf04150d401c23cdc792f'
const READY_TIMEOUT_MS = 5_000
const require = createRequire(import.meta.url)

const MODELS = [
  {
    id: 'standard',
    path: 'src-tauri/assets/models/standard/demomodel.moc3',
    sha256: '7bbcdb3df4fe085b0cbd9dc3a1cf32d351bd56787d0ddd1c238e50a5dcb6729a',
  },
  {
    id: 'keyboard',
    path: 'src-tauri/assets/models/keyboard/demomodel2.moc3',
    sha256: '03ed67f3ee2ea612aba4da0d42874f8879853d69043c9aae98af440d1f66965e',
  },
  {
    id: 'gamepad',
    path: 'src-tauri/assets/models/gamepad/demomodel3.moc3',
    sha256: 'e7f11d627011bb2c65d8b0882ce4545115d2256672dca256b674a713e3e5f3d6',
  },
]

const MOC_VERSION_NAMES = new Map([
  [0, 'MocVersion_Unknown'],
  [1, 'MocVersion_30'],
  [2, 'MocVersion_33'],
  [3, 'MocVersion_40'],
  [4, 'MocVersion_42'],
  [5, 'MocVersion_50'],
])

function sha256(buffer) {
  return crypto.createHash('sha256').update(buffer).digest('hex')
}

function readPinnedFile(relativePath, expectedHash) {
  const absolutePath = path.join(ROOT, relativePath)
  const buffer = fs.readFileSync(absolutePath)
  const actualHash = sha256(buffer)
  if (actualHash !== expectedHash) {
    throw new Error(
      `${relativePath} SHA-256 mismatch: expected ${expectedHash}, got ${actualHash}`,
    )
  }
  return buffer
}

function packedCoreVersion(raw) {
  return {
    raw,
    hex: `0x${raw.toString(16).padStart(8, '0')}`,
    display: `${(raw >>> 24) & 0xFF}.${(raw >>> 16) & 0xFF}.${raw & 0xFFFF}`,
  }
}

function mocVersion(raw) {
  const name = MOC_VERSION_NAMES.get(raw)
  if (name === undefined) {
    throw new Error(`legacy Core returned unknown MOC version enum ${raw}`)
  }
  return { raw, name }
}

function quietConsole(diagnostics) {
  return {
    log: (...values) => diagnostics.push(['log', ...values].join(' ')),
    warn: (...values) => diagnostics.push(['warn', ...values].join(' ')),
    error: (...values) => diagnostics.push(['error', ...values].join(' ')),
  }
}

async function loadLegacyCore() {
  const sourceBuffer = readPinnedFile(CORE_PATH, CORE_SHA256)
  const diagnostics = []
  const sandbox = {
    ArrayBuffer,
    Buffer,
    Float32Array,
    Int32Array,
    Promise,
    TextDecoder,
    TextEncoder,
    Uint8Array,
    Uint16Array,
    Uint32Array,
    WebAssembly,
    atob,
    btoa,
    clearTimeout,
    console: quietConsole(diagnostics),
    module: { exports: {} },
    exports: {},
    process,
    require,
    setTimeout,
    __dirname: path.dirname(path.join(ROOT, CORE_PATH)),
  }
  sandbox.global = sandbox

  vm.runInNewContext(sourceBuffer.toString('utf8'), sandbox, {
    filename: CORE_PATH,
    timeout: 2_000,
  })

  const deadline = Date.now() + READY_TIMEOUT_MS
  let lastError
  while (Date.now() < deadline) {
    try {
      const core = sandbox.Live2DCubismCore
      if (core?.Version?.csmGetVersion() > 0) {
        return { core, diagnostics }
      }
    } catch (error) {
      lastError = error
    }
    await new Promise(resolve => setTimeout(resolve, 10))
  }
  const detail = lastError instanceof Error ? `: ${lastError.message}` : ''
  throw new Error(`legacy Cubism Core did not initialize within ${READY_TIMEOUT_MS} ms${detail}`)
}

function inspectModel(core, definition) {
  const bytes = readPinnedFile(definition.path, definition.sha256)
  const arrayBuffer = bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  )
  const moc = core.Moc.fromArrayBuffer(arrayBuffer)
  if (moc === null) {
    throw new Error(`${definition.path} could not create a legacy Moc`)
  }

  let model
  try {
    const consistent = Boolean(moc.hasMocConsistency(arrayBuffer))
    const version = core.Version.csmGetMocVersion(moc, arrayBuffer)
    model = core.Model.fromMoc(moc)
    if (model === null) {
      throw new Error(`${definition.path} could not create a legacy Model`)
    }
    return {
      id: definition.id,
      mocPath: definition.path,
      mocSha256: definition.sha256,
      byteLength: bytes.byteLength,
      consistent,
      mocVersion: mocVersion(version),
      parameterCount: model.parameters.count,
      partCount: model.parts.count,
      drawableCount: model.drawables.count,
      canvas: {
        width: model.canvasinfo.CanvasWidth,
        height: model.canvasinfo.CanvasHeight,
        originX: model.canvasinfo.CanvasOriginX,
        originY: model.canvasinfo.CanvasOriginY,
        pixelsPerUnit: model.canvasinfo.PixelsPerUnit,
      },
    }
  } finally {
    model?.release()
    moc._release()
  }
}

async function buildSnapshot() {
  const { core, diagnostics } = await loadLegacyCore()
  const coreVersion = core.Version.csmGetVersion()
  const latestMocVersion = core.Version.csmGetLatestMocVersion()
  const models = MODELS.map(definition => inspectModel(core, definition))
  return {
    schemaVersion: 1,
    provenance: 'legacy_observation',
    source: {
      corePath: CORE_PATH,
      coreSha256: CORE_SHA256,
      coreVersion: packedCoreVersion(coreVersion),
      latestMocVersion: mocVersion(latestMocVersion),
    },
    models,
    capturedCoreLogLines: diagnostics.length,
  }
}

function canonicalJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`
}

async function main() {
  const args = process.argv.slice(2)
  if (args.length > 1 || (args.length === 1 && args[0] !== '--check')) {
    throw new Error(
      'usage: node tools/inspect-legacy-cubism-models.mjs [--check]',
    )
  }

  const snapshot = await buildSnapshot()
  if (args[0] !== '--check') {
    process.stdout.write(canonicalJson(snapshot))
    return
  }

  const expected = JSON.parse(fs.readFileSync(SNAPSHOT_PATH, 'utf8'))
  const expectedJson = canonicalJson(expected)
  const actualJson = canonicalJson(snapshot)
  if (actualJson !== expectedJson) {
    process.stderr.write('legacy Cubism baseline drifted\n')
    process.stderr.write(`expected:\n${expectedJson}actual:\n${actualJson}`)
    process.exitCode = 1
    return
  }
  process.stdout.write('legacy Cubism baseline matches\n')
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`legacy Cubism inspection failed: ${message}\n`)
  process.exitCode = 1
})
