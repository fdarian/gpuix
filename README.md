# Ghia

Build desktop apps with TypeScript on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's GPU-accelerated UI framework.

**Complete docs: [ghia.fdarian.com](https://ghia.fdarian.com)**

Ghia is a fork of [gpuix](https://github.com/remorses/gpuix) by [remorses](https://github.com/remorses), which uses the same rendering idea on top of GPUI. Ghia pushes how far GPUI can be a drop-in replacement for a web engine, so desktop apps get native performance while staying 99% compatible with the DOM and the TypeScript ecosystem.

## Before you use it

- Ghia is optimized for macOS and Solid. React and other platforms are not widely tested.
- Ghia is more opinionated and iterates much faster, so expect breaking changes. We recommend [gpuix](https://github.com/remorses/gpuix) for stability and a broader ecosystem. Also consider supporting the incredible work of gpuix.

## UI libraries

We recommend [fluid-functionalism-gpuix](https://github.com/fdarian/fluid-functionalism-gpuix) and [base-ui-solid-gpuix](https://github.com/fdarian/base-ui-solid-gpuix).

## What changed from gpuix

[docs/fork/index.md](docs/fork/index.md) lists the differences and how the fork syncs with upstream.

## License

[Apache 2.0](LICENSE). See [NOTICE](NOTICE).
