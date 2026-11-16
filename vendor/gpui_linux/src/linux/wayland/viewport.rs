//! Validate viewport destinations after conversion to protocol integer units.

pub(crate) fn destination(width: f32, height: f32) -> Option<(i32, i32)> {
    let (width, height) = (width as i32, height as i32);
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::destination;
    #[test]
    fn stretch_dimensions_wait_for_layer_surface_configuration() {
        assert_eq!(destination(0., 32.), None);
        assert_eq!(destination(0., 0.), None);
        assert_eq!(destination(2752., 0.), None);
        assert_eq!(destination(2752., 32.), Some((2752, 32)));
    }
    #[test]
    fn dimensions_are_valid_after_integer_conversion() {
        assert_eq!(destination(0.5, 32.), None);
        assert_eq!(destination(-1., 32.), None);
        assert_eq!(destination(640.5, 440.25), Some((640, 440)));
    }
}
