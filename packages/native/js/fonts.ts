export interface WindowFontsOption {
  /**
   * Font files (`.ttf`, `.otf`) registered with `registerFont` as soon as the
   * renderer exists, so `fontFamily` resolves to them without a system-wide
   * install. Relative paths resolve against the process's current working
   * directory. An unreadable path throws before any window opens.
   */
  fonts?: string[]
}

/** Read up front so a bad path fails before a window opens, naming the path. */
export function readFontFiles(paths: readonly string[] | undefined): Buffer[] {
  if (!paths) return []
  const fs = process.getBuiltinModule("node:fs")
  return paths.map((path) => {
    try {
      return fs.readFileSync(path)
    } catch (error) {
      throw new Error(`Failed to read font file ${path}: ${String(error)}`, {
        cause: error,
      })
    }
  })
}
