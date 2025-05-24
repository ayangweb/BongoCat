export function isImage(value: string) {
  const regex = /\.(?:jpe?g|png|webp|avif|gif|svg|bmp|ico|tiff?|heic|apng)$/i

  return regex.test(value)
}
