export function clearObject(target: Record<string, unknown>) {
  for (const key of Object.keys(target)) {
    delete target[key]
  }
}
