export function getImageSize(src: string) {
  return new Promise<{ width: number, height: number }>((resolve, reject) => {
    const img = new Image()

    img.src = src

    img.onload = () => {
      const { naturalWidth, naturalHeight } = img

      resolve({ width: naturalWidth, height: naturalHeight })
    }

    img.onerror = reject
  })
}
