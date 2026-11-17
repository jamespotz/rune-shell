//! GPUI layer-shell configuration, shared by the window registry.

use crate::Surface;
use gpui::{
    Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point, px, size,
};

/// For panels, `margin: Some` means explicit left placement, including zero.
/// Centered panels use `None`; their top gap is supplied here.
pub(crate) fn options(
    surface: &Surface,
    dimensions: (u32, u32),
    margin: Option<(i32, i32, i32, i32)>,
    grabs_keyboard: bool,
) -> WindowOptions {
    let (layer, anchor, exclusive_zone, keyboard_interactivity) = match surface {
        Surface::Bar(_) => (
            Layer::Top,
            Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            Some(px(
                (dimensions.1 as i32 + margin.map_or(0, |(_, _, gap, _)| gap.max(0))) as f32,
            )),
            KeyboardInteractivity::None,
        ),
        Surface::Catcher => (Layer::Top, Anchor::all(), None, KeyboardInteractivity::None),
        Surface::Panel => (
            Layer::Overlay,
            if grabs_keyboard {
                Anchor::empty()
            } else if margin.is_some() {
                Anchor::TOP | Anchor::LEFT
            } else {
                Anchor::TOP
            },
            None,
            if grabs_keyboard {
                KeyboardInteractivity::Exclusive
            } else {
                KeyboardInteractivity::OnDemand
            },
        ),
        Surface::Settings => (
            Layer::Overlay,
            Anchor::empty(),
            None,
            KeyboardInteractivity::OnDemand,
        ),
        Surface::Popups => (
            Layer::Overlay,
            Anchor::TOP | Anchor::RIGHT,
            None,
            KeyboardInteractivity::None,
        ),
        Surface::Osd => (
            Layer::Overlay,
            Anchor::BOTTOM,
            None,
            KeyboardInteractivity::None,
        ),
    };
    WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(dimensions.0 as f32), px(dimensions.1 as f32)),
        })),
        app_id: Some("rune-shell".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: surface.namespace().into(),
            layer,
            anchor,
            exclusive_zone,
            keyboard_interactivity,
            margin: margin
                .filter(|_| !matches!(surface, Surface::Bar(_)))
                .or_else(|| {
                    matches!(surface, Surface::Panel).then_some((crate::panel::GAP, 0, 0, 0))
                })
                .map(|(t, r, b, l)| (px(t as f32), px(r as f32), px(b as f32), px(l as f32))),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Owns the actual GPUI windows. Entries are removed before closing, so callbacks
/// can safely request the same surface again.
#[derive(Default)]
pub(crate) struct Windows {
    handles: std::collections::HashMap<Surface, gpui::AnyWindowHandle>,
}

impl Windows {
    pub fn open<V: gpui::Render + 'static>(
        &mut self,
        surface: Surface,
        options: WindowOptions,
        cx: &mut gpui::App,
        build: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> gpui::Entity<V>,
    ) -> anyhow::Result<()> {
        if let Some(handle) = self.handles.get(&surface).copied() {
            if handle.update(cx, |_, _, _| ()).is_ok() {
                return Ok(());
            }
            self.handles.remove(&surface);
        }
        let input_transparent = surface == Surface::Osd;
        let handle = cx.open_window(options, |window, cx| {
            if input_transparent {
                window.set_input_region(Some(&[]));
            }
            build(window, cx)
        })?;
        self.handles.insert(surface, handle.into());
        Ok(())
    }

    pub fn close(&mut self, surface: &Surface, cx: &mut gpui::App) {
        if let Some(handle) = self.handles.remove(surface) {
            // An externally closed window is already in the desired state.
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    pub fn resize(&mut self, surface: &Surface, dimensions: (u32, u32), cx: &mut gpui::App) {
        if let Some(handle) = self.handles.get(surface).copied()
            && handle
                .update(cx, |_, window, _| {
                    window.resize(size(px(dimensions.0 as f32), px(dimensions.1 as f32)));
                })
                .is_err()
        {
            self.handles.remove(surface);
        }
    }

    pub fn retain(&mut self, mut wanted: impl FnMut(&Surface) -> bool, cx: &mut gpui::App) {
        let obsolete: Vec<_> = self
            .handles
            .keys()
            .filter(|s| !wanted(s))
            .cloned()
            .collect();
        for surface in obsolete {
            self.close(&surface, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Surface;
    use gpui::{
        WindowKind,
        layer_shell::{Anchor, KeyboardInteractivity, Layer},
        px,
    };

    #[test]
    fn bars_reserve_space_without_keyboard_focus() {
        let options = options(&Surface::Bar("DP-1".into()), (0, 32), None, false);
        let WindowKind::LayerShell(layer) = options.kind else {
            panic!("expected layer shell")
        };
        assert_eq!(layer.namespace, "rune-bar");
        assert_eq!(layer.layer, Layer::Top);
        assert_eq!(layer.anchor, Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        assert_eq!(layer.exclusive_zone, Some(px(32.)));
        assert_eq!(layer.keyboard_interactivity, KeyboardInteractivity::None);
    }

    #[test]
    fn bar_gap_extends_reserved_space() {
        let options = options(
            &Surface::Bar("DP-1".into()),
            (0, 32),
            Some((0, 0, 8, 0)),
            false,
        );
        let WindowKind::LayerShell(layer) = options.kind else {
            panic!("expected layer shell")
        };
        assert_eq!(layer.exclusive_zone, Some(px(40.)));
    }

    #[test]
    fn keyboard_panels_center_on_screen() {
        let options = options(&Surface::Panel, (640, 440), None, true);
        let WindowKind::LayerShell(layer) = options.kind else {
            panic!("expected layer shell")
        };
        assert_eq!(layer.anchor, Anchor::empty());
    }

    #[test]
    fn launcher_grabs_keyboard_but_settings_does_not() {
        let options = options(&Surface::Panel, (640, 440), None, true);
        let WindowKind::LayerShell(layer) = options.kind else {
            panic!("expected layer shell")
        };
        assert_eq!(layer.anchor, Anchor::empty());
        assert_eq!(
            layer.keyboard_interactivity,
            KeyboardInteractivity::Exclusive
        );
        let settings = options_for_settings();
        assert_eq!(
            settings.keyboard_interactivity,
            KeyboardInteractivity::OnDemand
        );
    }

    #[test]
    fn panel_at_left_edge_keeps_its_explicit_anchor() {
        let WindowKind::LayerShell(layer) =
            options(&Surface::Panel, (640, 440), Some((8, 0, 0, 0)), false).kind
        else {
            panic!("expected layer shell")
        };
        assert_eq!(layer.anchor, Anchor::TOP | Anchor::LEFT);
    }

    fn options_for_settings() -> gpui::layer_shell::LayerShellOptions {
        let WindowKind::LayerShell(layer) =
            options(&Surface::Settings, (860, 580), None, false).kind
        else {
            panic!("expected layer shell")
        };
        layer
    }
}
