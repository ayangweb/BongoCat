export function getImageSize(path: string): Promise<{ width: number, height: number }> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.src = path

    img.onload = () => {
      const { naturalWidth, naturalHeight } = img

      resolve({
        width: naturalWidth,
        height: naturalHeight,
      })
    }

    img.onerror = (error) => {
      reject(error)
    }
  })
}
