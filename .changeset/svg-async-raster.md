---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

A large `<svg>` is rasterised on a background thread, so a size, colour or document change no longer costs a frame (a 3000-shape, 1200x1200 SVG stalled the frame for about 1.7 s; the longest frame is now about 1.4 ms). The previous raster keeps being drawn, stretched to the box, until the new one lands and the view repaints. A large `<svg>` draws nothing on its first paint; one up to about 4 KB of markup at up to 128x128 device pixels is still rasterised in paint. The 100 ms resize throttle is gone. The test renderer waits for pending rasters.
