# Image memory optimization

The release build was measured on the same Hyprland session before and after the
image changes. Values below are proportional resident RAM from
`/proc/<pid>/smaps_rollup`, expressed in MiB. They are not GPU-memory totals.

| State | Before PSS | After PSS |
| --- | ---: | ---: |
| idle | 99.4 | 103.5 |
| launcher open | 206.0 | 135.0 |
| launcher closed | 206.5 | 135.0 |
| settings open | 207.1 | 135.4 |
| settings closed | 207.1 | 135.4 |

Each run started the release daemon, waited five seconds, opened and closed the
launcher and settings through the normal CLI, waited two seconds after each
transition, then quit normally. These are single-run comparisons, not a statistical
benchmark. Idle did not improve; shared mappings, live service data, driver pools
and allocator retention can vary between runs. The launcher-open PSS reduction
was about 71 MiB, or 34%.

## Changes

- Decode album art at at most 512 pixels on either edge, and icons/notification
  pictures at at most 128 pixels, preserving aspect ratio. Avoid an extra full-size
  RGBA-to-BGRA copy when converting owned decoded pixels.
- Share decoded images across bar/player and popup/history views. The lookup
  index owns weak references, while each view owns only the images it uses.
- Load file and SVG thumbnails on GPUI’s background executor. SVG rasterization
  uses the requested thumbnail bounds rather than the file’s intrinsic size.
  Animated GIF/WebP thumbnails preserve frames and timing.
- Explicitly evict retired GPUI file assets during replacement/pruning and view
  closure. A remaining view lease prevents eviction of an image still in use.
- Normalize notification image-data to thumbnails before retaining it in history.
  A 4096×4096 RGBA notification previously required 64 MiB of pixel data; its
  retained 128×128 thumbnail requires 64 KiB. A square 4096px album image now
  reaches the renderer as at most 1 MiB of decoded pixels instead of 64 MiB.

Large compressed raster files can still require transient full-size decode buffers.
This pass bounds retained/rendered pixels, not every codec’s transient allocation.
It does not change full-screen catcher allocation, wgpu buffer pools or renderer
selection. These remain separate candidates for further measured work.

## Verification

- `cargo test --workspace --no-fail-fast`: 125 passed; ten native tests ignored by default.
- All ten native tests run individually: passed, including real GPUI asset eviction.
- `cargo build --release`, formatting checks and `git diff --check`: passed.
- Regression tests cover source replacement, weak-owner release, cross-view reuse,
  thumbnail dimensions/aspect ratio, SVG sizing, animated frame timing, image alpha,
  notification history pixels, and retirement without evicting another view’s image.
- Independent review found the long-lived-view retirement issue; the final fix
  was covered by model and native cache-lifetime tests.
