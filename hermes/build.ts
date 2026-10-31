/**
 * Bundle GPUIX apps to CJS for hermes-node. Hermes has no ESM, so dynamic
 * import() in the automation client is stubbed out.
 */
import * as esbuild from 'esbuild'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname ?? path.dirname(new URL(import.meta.url).pathname))
const repo = path.resolve(root, '..')
const shim = path.join(root, 'shim-automation.js')
const target = process.argv[2] === 'chat' ? 'chat' : 'app'

const common: esbuild.BuildOptions = {
  absWorkingDir: repo,
  bundle: true,
  platform: 'node',
  format: 'cjs',
  external: ['@ghia/native'],
  jsx: 'automatic',
  jsxImportSource: '@ghia/react',
  define: { 'process.env.NODE_ENV': '"production"' },
  alias: {
    '@ghia/react': path.join(repo, 'packages/react'),
    react: path.join(repo, 'node_modules/react'),
  },
  plugins: [
    {
      name: 'stub-automation',
      setup(build) {
        build.onResolve({ filter: /automation\/client/ }, () => ({ path: shim }))
      },
    },
    {
      name: 'stub-safe-mdx',
      setup(build) {
        const stub = path.join(root, 'shim-safe-mdx.js')
        build.onResolve({ filter: /^safe-mdx(\/|$)/ }, () => ({ path: stub }))
      },
    },
    {
      name: 'svg-as-text',
      setup(build) {
        build.onLoad({ filter: /\.svg$/ }, (args) => ({
          contents: fs.readFileSync(args.path, 'utf8'),
          loader: 'text',
        }))
      },
    },
  ],
  banner: {
    js: 'if(typeof queueMicrotask!=="function"){globalThis.queueMicrotask=function(fn){process.nextTick(fn)};}if(typeof performance==="undefined"){globalThis.performance={now:function(){var t=process.hrtime();return t[0]*1e3+t[1]/1e6;}}}',
  },
  logLevel: 'info',
}

await esbuild.build({
  ...common,
  entryPoints: [path.join(root, `${target}.tsx`)],
  outfile: path.join(root, `${target}.cjs`),
})
