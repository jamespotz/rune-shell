//! Bounded image decoding shared across views, with weak cache ownership.

use gpui::{Asset, ImageCacheError, ImageSource, RenderImage};
use rune_services::notifications::Image;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    path::PathBuf,
    rc::{Rc, Weak},
    sync::Arc,
};

const ICON_EDGE: u32 = 128;
const ART_EDGE: u32 = 512;

fn pixels(image: image::DynamicImage, edge: u32) -> image::RgbaImage {
    let image = if image.width() > edge || image.height() > edge {
        image.thumbnail(edge, edge)
    } else {
        image
    };
    let mut pixels = image.into_rgba8();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    pixels
}
fn render(image: image::DynamicImage, edge: u32) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![image::Frame::new(pixels(
        image, edge,
    ))]))
}
fn rgba(width: u32, height: u32, bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let expected = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if width == 0 || height == 0 || expected != bytes.len() {
        return None;
    }
    let pixels = image::RgbaImage::from_raw(width, height, bytes.to_vec())?;
    Some(render(image::DynamicImage::ImageRgba8(pixels), ICON_EDGE))
}

#[derive(Clone, PartialEq, Eq)]
enum Source {
    Raw(Image),
    Encoded(Arc<[u8]>, u32),
}
impl Source {
    fn hash(&self) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        match self {
            Self::Raw(Image::Path(path)) => {
                0u8.hash(&mut hash);
                path.hash(&mut hash);
            }
            Self::Raw(Image::Rgba {
                width,
                height,
                data,
            }) => {
                1u8.hash(&mut hash);
                width.hash(&mut hash);
                height.hash(&mut hash);
                data.hash(&mut hash);
            }
            Self::Encoded(bytes, edge) => {
                2u8.hash(&mut hash);
                edge.hash(&mut hash);
                bytes.hash(&mut hash);
            }
        }
        hash.finish()
    }
}
struct Cached {
    source: Source,
    image: Option<ImageSource>,
}
type Shared = Rc<RefCell<HashMap<u64, Vec<Weak<Cached>>>>>;

#[derive(Default)]
pub(crate) struct Images {
    shared: Shared,
    entries: HashMap<String, Rc<Cached>>,
    assets: HashSet<(PathBuf, u32)>,
}
// A new view shares the lookup index, not another view's strong pixel ownership.
impl Clone for Images {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            entries: HashMap::new(),
            assets: HashSet::new(),
        }
    }
}
impl Images {
    fn get(&mut self, key: &str, source: Source) -> Option<ImageSource> {
        if let Some(cached) = self.entries.get(key)
            && cached.source == source
        {
            return cached.image.clone();
        }
        let fingerprint = source.hash();
        let reused = self.shared.borrow().get(&fingerprint).and_then(|bucket| {
            bucket
                .iter()
                .filter_map(Weak::upgrade)
                .find(|cached| cached.source == source)
        });
        let cached = reused.unwrap_or_else(|| {
            let image = match &source {
                Source::Raw(Image::Path(path)) => {
                    let request = (path.clone(), ICON_EDGE);
                    Some(ImageSource::Custom(Arc::new(move |window, cx| {
                        window.use_asset::<Thumbnail>(&request, cx)
                    })))
                }
                Source::Raw(Image::Rgba {
                    width,
                    height,
                    data,
                }) => rgba(*width, *height, data).map(ImageSource::from),
                Source::Encoded(bytes, edge) => image::load_from_memory(bytes)
                    .ok()
                    .map(|image| ImageSource::from(render(image, *edge))),
            };
            let cached = Rc::new(Cached { source, image });
            let mut shared = self.shared.borrow_mut();
            shared.retain(|_, bucket| {
                bucket.retain(|entry| entry.strong_count() > 0);
                !bucket.is_empty()
            });
            shared
                .entry(fingerprint)
                .or_default()
                .push(Rc::downgrade(&cached));
            cached
        });
        let image = cached.image.clone();
        self.entries.insert(key.to_owned(), cached);
        image
    }
    pub fn source(&mut self, key: &str, source: &Image) -> Option<ImageSource> {
        if let Image::Path(path) = source {
            self.assets.insert((path.clone(), ICON_EDGE));
        }
        self.get(key, Source::Raw(source.clone()))
    }
    pub fn encoded(&mut self, key: &str, bytes: &Arc<[u8]>) -> Option<ImageSource> {
        self.get(key, Source::Encoded(bytes.clone(), ART_EDGE))
    }
    pub fn encoded_icon(&mut self, key: &str, bytes: &Arc<[u8]>) -> Option<ImageSource> {
        self.get(key, Source::Encoded(bytes.clone(), ICON_EDGE))
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.entries.retain(|key, _| keep(key));
        self.shared.borrow_mut().retain(|_, bucket| {
            bucket.retain(|entry| entry.strong_count() > 0);
            !bucket.is_empty()
        });
    }
    fn unused_assets(&self) -> Vec<(PathBuf, u32)> {
        self.assets.iter().filter(|(path, _)| !self.entries.values().any(|cached| matches!(&cached.source, Source::Raw(Image::Path(active)) if active == path))).cloned().collect()
    }
    pub fn release_unused_assets(&mut self, cx: &mut gpui::App) {
        for request in self.unused_assets() {
            if self.can_release_asset(&request) {
                cx.remove_asset::<Thumbnail>(&request);
            }
            self.assets.remove(&request);
        }
    }
    fn can_release_asset(&self, request: &(PathBuf, u32)) -> bool {
        let source = Source::Raw(Image::Path(request.0.clone()));
        let local = self
            .entries
            .values()
            .filter(|cached| cached.source == source)
            .count();
        let shared = self.shared.borrow();
        let live = shared
            .get(&source.hash())
            .and_then(|bucket| {
                bucket
                    .iter()
                    .filter_map(Weak::upgrade)
                    .find(|cached| cached.source == source)
            })
            .map_or(0, |cached| Rc::strong_count(&cached) - 1);
        live <= local
    }
    pub fn release_assets(&mut self, cx: &mut gpui::App) {
        for request in std::mem::take(&mut self.assets) {
            if self.can_release_asset(&request) {
                cx.remove_asset::<Thumbnail>(&request);
            }
        }
    }
}

struct Thumbnail;
impl Asset for Thumbnail {
    type Source = (PathBuf, u32);
    type Output = Result<Arc<RenderImage>, ImageCacheError>;
    fn load(
        (path, edge): Self::Source,
        cx: &mut gpui::App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let svg = cx.svg_renderer();
        async move {
            // GPUI executes Asset loads on its background executor.
            thumbnail(&std::fs::read(path)?, edge, &svg)
        }
    }
}

fn thumbnail(
    bytes: &[u8],
    edge: u32,
    svg: &gpui::SvgRenderer,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    use image::AnimationDecoder;
    let frames = match image::guess_format(bytes) {
        Ok(image::ImageFormat::Gif) => {
            Some(image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))?.into_frames())
        }
        Ok(image::ImageFormat::WebP) => {
            let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))?;
            if decoder.has_animation() {
                Some(decoder.into_frames())
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(frames) = frames {
        let frames = frames
            .map(|frame| {
                frame.map(|frame| {
                    let delay = frame.delay();
                    let pixels = pixels(image::DynamicImage::ImageRgba8(frame.into_buffer()), edge);
                    image::Frame::from_parts(pixels, 0, 0, delay)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Arc::new(RenderImage::new(frames)));
    }
    if image::guess_format(bytes).is_ok() {
        Ok(render(image::load_from_memory(bytes)?, edge))
    } else {
        let parsed = svg.parse_svg(bytes)?;
        let edge_pixels = i32::try_from(edge).unwrap_or(i32::MAX);
        svg.render_parsed(
            &parsed,
            gpui::SvgSize::Size(gpui::size(
                gpui::DevicePixels(edge_pixels),
                gpui::DevicePixels(edge_pixels),
            )),
        )
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgba_is_converted_to_bgra_without_changing_alpha() {
        let image = rgba(1, 1, &[200, 30, 10, 128]).unwrap();
        assert_eq!(image.as_bytes(0).unwrap(), [10, 30, 200, 128]);
        assert!(rgba(2, 2, &[0; 4]).is_none());
    }
    #[test]
    fn identical_source_reuses_image_and_changed_source_replaces_it() {
        let mut cache = Images::default();
        let source = rune_services::notifications::Image::Rgba {
            width: 1,
            height: 1,
            data: std::sync::Arc::from([1, 2, 3, 255]),
        };
        let a = cache.source("tray", &source).unwrap();
        let b = cache.source("tray", &source).unwrap();
        let (gpui::ImageSource::Render(a), gpui::ImageSource::Render(b)) = (a, b) else {
            panic!("render image expected")
        };
        assert_eq!(a.id, b.id);
        let changed = rune_services::notifications::Image::Rgba {
            width: 1,
            height: 1,
            data: std::sync::Arc::from([3, 2, 1, 255]),
        };
        let gpui::ImageSource::Render(c) = cache.source("tray", &changed).unwrap() else {
            panic!("render image expected")
        };
        assert_ne!(a.id, c.id);
    }
    #[test]
    fn large_images_are_bounded_before_reaching_the_renderer() {
        let source = vec![255; 1024 * 512 * 4];
        let image = rgba(1024, 512, &source).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (128, 64));
        assert_eq!(image.as_bytes(0).unwrap().len(), 128 * 64 * 4);
    }

    #[test]
    fn views_share_decodes_but_closed_views_do_not_pin_them() {
        let mut bar = Images::default();
        let mut player = bar.clone();
        let pixels = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1024,
            512,
            image::Rgba([220, 20, 40, 255]),
        ));
        let mut png = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes: Arc<[u8]> = png.into_inner().into();
        let ImageSource::Render(a) = bar.encoded("bar-art", &bytes).unwrap() else {
            panic!("render image expected")
        };
        let ImageSource::Render(b) = player.encoded("player-art", &bytes).unwrap() else {
            panic!("render image expected")
        };
        assert_eq!(a.id, b.id);
        assert_eq!((a.size(0).width.0, a.size(0).height.0), (512, 256));
        let weak = Arc::downgrade(&a);
        drop(a);
        drop(b);
        bar.retain(|_| false);
        assert!(weak.upgrade().is_some(), "player must keep its image alive");
        drop(player);
        assert!(
            weak.upgrade().is_none(),
            "shared index must not retain unused pixels"
        );
    }
    #[test]
    fn vector_thumbnails_render_at_the_limit_not_the_svg_intrinsic_size() {
        let svg = gpui::SvgRenderer::new(Arc::new(()));
        let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4096" height="2048"><rect width="4096" height="2048" fill="red"/></svg>"#;
        let image = thumbnail(bytes, ICON_EDGE, &svg).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (128, 64));
    }
    #[test]
    fn animated_thumbnails_preserve_frames_and_timing() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            let frames =
                [([255, 0, 0, 255], 100), ([0, 0, 255, 255], 200)].map(|(color, delay)| {
                    image::Frame::from_parts(
                        image::RgbaImage::from_pixel(256, 128, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(delay, 1),
                    )
                });
            encoder.encode_frames(frames).unwrap();
        }
        let svg = gpui::SvgRenderer::new(Arc::new(()));
        let image = thumbnail(&bytes, ICON_EDGE, &svg).unwrap();
        assert_eq!(image.frame_count(), 2);
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (128, 64));
        assert_eq!(image.delay(0).numer_denom_ms(), (100, 1));
        assert_eq!(image.delay(1).numer_denom_ms(), (200, 1));
    }
    #[test]
    fn closing_a_view_does_not_evict_another_views_file_thumbnail() {
        let mut bar = Images::default();
        let mut player = bar.clone();
        let path: PathBuf = "shared.png".into();
        let source = Image::Path(path.clone());
        bar.source("bar", &source);
        player.source("player", &source);
        assert!(!bar.can_release_asset(&(path.clone(), ICON_EDGE)));
        drop(player);
        assert!(bar.can_release_asset(&(path, ICON_EDGE)));
    }
    #[test]
    fn replacement_and_pruning_identify_retired_file_assets() {
        let mut images = Images::default();
        images.source("tray", &Image::Path("old.png".into()));
        images.source("tray", &Image::Path("new.png".into()));
        assert_eq!(
            images.unused_assets(),
            vec![(PathBuf::from("old.png"), ICON_EDGE)]
        );
        images.retain(|_| false);
        let mut retired = images.unused_assets();
        retired.sort();
        assert_eq!(
            retired,
            vec![
                (PathBuf::from("new.png"), ICON_EDGE),
                (PathBuf::from("old.png"), ICON_EDGE)
            ]
        );
    }
    #[test]
    #[ignore = "requires native GPUI application initialization"]
    fn retired_file_assets_are_removed_from_the_gpui_cache() {
        use gpui::QuitMode;
        let path =
            std::env::temp_dir().join(format!("rune-thumbnail-cache-{}.svg", std::process::id()));
        std::fs::write(&path, br#"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="512"><rect width="1024" height="512" fill="red"/></svg>"#).unwrap();
        let test_path = path.clone();
        gpui_platform::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(move |cx| {
                let mut first = Images::default();
                let mut second = first.clone();
                let source = Image::Path(test_path.clone());
                first.source("tray", &source);
                second.source("popup", &source);
                let request = (test_path, ICON_EDGE);
                cx.spawn(async move |cx| {
                    let started = std::time::Instant::now();
                    let image = loop {
                        if let Some(image) = cx.update(|cx| cx.fetch_asset::<Thumbnail>(&request)) {
                            break image.unwrap();
                        }
                        assert!(
                            started.elapsed() < std::time::Duration::from_secs(5),
                            "thumbnail load timed out"
                        );
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(10))
                            .await;
                    };
                    cx.update(|cx| {
                        first.retain(|_| false);
                        first.release_unused_assets(cx);
                        let retained = cx
                            .fetch_asset::<Thumbnail>(&request)
                            .expect("another view still needs this thumbnail")
                            .unwrap();
                        assert_eq!(image.id, retained.id);
                        second.retain(|_| false);
                        second.release_unused_assets(cx);
                        assert!(
                            cx.fetch_asset::<Thumbnail>(&request).is_none(),
                            "last retired lease must evict the GPUI asset"
                        );
                        cx.quit();
                    });
                })
                .detach();
            });
        std::fs::remove_file(path).unwrap();
    }
}
