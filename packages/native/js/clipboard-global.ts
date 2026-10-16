// Kept apart from clipboard.ts, where the exported `Clipboard` interface would
// shadow the global one named here. `Navigator.clipboard` must be typed with the
// global `Clipboard` so it stays identical to lib.dom's declaration (no TS2717)
// in programs that have the DOM lib, and the method overloads merge with it.
declare global {
  interface Clipboard {
    writeText(text: string): Promise<void>
    readText(): Promise<string>
  }
  interface Navigator {
    readonly clipboard: Clipboard
  }
}

export {}
