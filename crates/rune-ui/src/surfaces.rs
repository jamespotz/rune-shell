//! Toolkit-independent desired shell surfaces.
pub use crate::surface::Surface;
use crate::{Message, backend::Effects};
use std::collections::HashMap;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum OutputOption {
    #[default]
    Active,
    OutputName(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WindowSpec {
    pub size: Option<(u32, u32)>,
    pub margin: Option<(i32, i32, i32, i32)>,
    pub output_option: OutputOption,
    pub grabs_keyboard: bool,
}
#[derive(Debug, Default)]
pub struct Surfaces {
    pub(crate) desired: HashMap<Surface, WindowSpec>,
}
impl Surfaces {
    pub(crate) fn open(&mut self, surface: Surface, spec: WindowSpec) -> Effects<Message> {
        self.desired.entry(surface).or_insert(spec);
        Effects::none()
    }
    pub(crate) fn close(&mut self, surface: &Surface) -> Effects<Message> {
        self.desired.remove(surface);
        Effects::none()
    }
    pub(crate) fn resize(&mut self, surface: &Surface, size: (u32, u32)) -> Effects<Message> {
        if let Some(spec) = self.desired.get_mut(surface) {
            spec.size = Some(size);
        }
        Effects::none()
    }
    pub(crate) fn margin(
        &mut self,
        surface: &Surface,
        margin: (i32, i32, i32, i32),
    ) -> Effects<Message> {
        if let Some(spec) = self.desired.get_mut(surface) {
            spec.margin = Some(margin);
        }
        Effects::none()
    }
    pub fn is_open(&self, surface: &Surface) -> bool {
        self.desired.contains_key(surface)
    }
    pub fn bars(&self) -> Vec<String> {
        self.desired
            .keys()
            .filter_map(|s| {
                if let Surface::Bar(m) = s {
                    Some(m.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desired_specs_follow_open_resize_and_close() {
        let mut surfaces = Surfaces::default();
        surfaces.open(
            Surface::Bar("DP-1".into()),
            WindowSpec {
                size: Some((0, 32)),
                ..Default::default()
            },
        );
        surfaces.open(Surface::Panel, WindowSpec::default());
        surfaces.resize(&Surface::Panel, (480, 300));
        surfaces.margin(&Surface::Panel, (8, 0, 0, 0));
        assert_eq!(surfaces.bars(), ["DP-1"]);
        assert_eq!(surfaces.desired[&Surface::Panel].size, Some((480, 300)));
        assert_eq!(surfaces.desired[&Surface::Panel].margin, Some((8, 0, 0, 0)));
        surfaces.close(&Surface::Panel);
        assert!(!surfaces.is_open(&Surface::Panel));
    }
}
