/// Desktop checkUpdate() against a local JSON feed.

import { createServer, type IncomingMessage, type ServerResponse } from "node:http"
import { createRequire } from "node:module"
import { afterEach, describe, expect, it } from "vitest"

type CheckUpdateOptions = {
  endpoints: string[]
  pubkey: string
  timeoutMs?: number
}

type AvailableUpdate = {
  version: string
  currentVersion: string
  notes?: string | null
  downloadUrl: string
  format: string
}

type NativeUpdater = {
  checkUpdate?: (
    currentVersion: string,
    options: CheckUpdateOptions,
  ) => Promise<AvailableUpdate | null>
}

const native = createRequire(import.meta.url)("@ghia/native") as NativeUpdater
const describeNative = typeof native.checkUpdate === "function" ? describe : describe.skip

function listen(
  handler: (req: IncomingMessage, res: ServerResponse) => void,
): Promise<{ url: string; close: () => Promise<void> }> {
  const server = createServer(handler)
  return new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address()
      if (!address || typeof address === "string") {
        reject(new Error("expected a TCP address"))
        return
      }
      resolve({
        url: `http://127.0.0.1:${address.port}`,
        close: () =>
          new Promise((closeResolve, closeReject) => {
            server.close((err) => (err ? closeReject(err) : closeResolve()))
          }),
      })
    })
  })
}

describeNative("checkUpdate()", () => {
  let close: (() => Promise<void>) | undefined

  afterEach(async () => {
    await close?.()
    close = undefined
  })

  it("returns null on 204", async () => {
    const server = await listen((_req, res) => {
      res.statusCode = 204
      res.end()
    })
    close = server.close
    const update = await native.checkUpdate!("0.1.0", {
      endpoints: [server.url],
      pubkey: "not-a-real-key",
      timeoutMs: 2000,
    })
    expect(update).toBeNull()
  })

  it("returns metadata when the feed version is newer", async () => {
    const server = await listen((_req, res) => {
      res.setHeader("content-type", "application/json")
      res.end(
        JSON.stringify({
          version: "0.2.0",
          notes: "bugfix",
          url: "http://127.0.0.1/MyApp.app.tar.gz",
          signature: "sig",
          format: "app",
        }),
      )
    })
    close = server.close
    const update = await native.checkUpdate!("0.1.0", {
      endpoints: [server.url],
      pubkey: "not-a-real-key",
      timeoutMs: 2000,
    })
    expect(update).not.toBeNull()
    expect(update!.version).toBe("0.2.0")
    expect(update!.currentVersion).toBe("0.1.0")
    expect(update!.notes).toBe("bugfix")
    expect(update!.format).toBe("app")
    expect(update!.downloadUrl).toBe("http://127.0.0.1/MyApp.app.tar.gz")
  })

  it("reads GitHub latest JSON and the sibling .sig", async () => {
    const bundle =
      process.platform === "darwin"
        ? "My App.app.tar.gz"
        : process.platform === "win32"
          ? `app_0.2.0_${process.arch === "arm64" ? "arm64" : "x64"}-setup.exe`
          : `app_0.2.0_${process.arch === "arm64" ? "aarch64" : "x86_64"}.AppImage`
    let origin = ""
    const server = await listen((req, res) => {
      if (req.url === "/" || req.url === "/repos/OWNER/REPO/releases/latest") {
        res.setHeader("content-type", "application/json")
        res.end(
          JSON.stringify({
            tag_name: "v0.2.0",
            body: "bugfix",
            published_at: "2026-09-07T12:00:00Z",
            assets: [
              {
                name: bundle,
                browser_download_url: `${origin}/bundle`,
              },
              {
                name: `${bundle}.sig`,
                browser_download_url: `${origin}/bundle.sig`,
              },
            ],
          }),
        )
        return
      }
      if (req.url === "/bundle.sig") {
        res.end("untrusted comment: signature\nRWQ=\n")
        return
      }
      res.statusCode = 404
      res.end()
    })
    origin = server.url
    close = server.close
    const update = await native.checkUpdate!("0.1.0", {
      endpoints: [server.url],
      pubkey: "not-a-real-key",
      timeoutMs: 2000,
    })
    expect(update).not.toBeNull()
    expect(update!.version).toBe("0.2.0")
    expect(update!.notes).toBe("bugfix")
    expect(update!.downloadUrl).toBe(`${server.url}/bundle`)
  })
})
