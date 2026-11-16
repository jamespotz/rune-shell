# Rune's GPUI Linux patch

Source: Zed revision `917559da35dd4d626205da705b071c800c9c2553`,
`crates/gpui_linux`, licensed under Apache-2.0.

The crate is vendored because a layer-shell window can receive fractional scale
before its first configure event resolves zero stretch dimensions. Upstream then
sends `wp_viewport.set_destination(0, height)`, which Hyprland rejects with
`Size was <= 0`, terminating the Wayland connection.

`window.rs` now skips destination updates until both dimensions are positive
protocol integers. The original layer-shell stretch request is unchanged; the
first valid configure event supplies the viewport destination. This applies to
zero-width bars and zero-sized click catchers without guessing an output size.
`viewport.rs` contains the validator and its regression tests. `wayland.rs`
registers that helper module.

Cargo workspace inheritance has been expanded in this manifest. Internal Zed
packages still use the same pinned git revision; native dependencies retain
upstream versions and features. The upstream workspace's `unexpected_cfgs` lint
setting is retained explicitly. The root manifest patches only `gpui_linux`.

Run the focused, dependency-free regression tests with:

```sh
rustc --test vendor/gpui_linux/src/linux/wayland/viewport.rs -o /tmp/rune-viewport-tests
/tmp/rune-viewport-tests
```

Also run the workspace suite and the release binary on fractional-scale Wayland.
The original failure is observable with `WAYLAND_DEBUG=client`: a zero viewport
destination is sent before the layer-surface configure event.

Remove this local patch when the pinned upstream implementation includes the
same protection and passes the release startup reproduction.
