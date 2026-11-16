softbuffer 0.4.8 from crates.io, unchanged except `src/backends/wayland/buffer.rs`:
`wl_shm::Format::Xrgb8888` → `Argb8888` (two places), so Wayland honours the alpha
that tiny-skia writes. Used through `[patch.crates-io]` in the workspace Cargo.toml.
