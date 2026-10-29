//! Decode the three preview planes off the UI thread.

/// A frame-sized edit plane loses everything outside the frame. Only reuse
/// it for local transforms when the entire source element fits on the frame.
pub fn can_transform_locally(rect: super::placement::Rect, width: f32, height: f32) -> bool {
    rect.x >= 0.0 && rect.y >= 0.0 && rect.w > 0.0 && rect.h > 0.0 && rect.x + rect.w <= width && rect.y + rect.h <= height
}

pub struct EditPreview {
    pub full: crate::net::Frame,
    pub planes: Vec<crate::net::Frame>,
}

pub fn edit_preview(bytes: &[u8], atlas_height: u32) -> Option<EditPreview> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let as_frame = |rgba: image::RgbaImage| crate::net::Frame { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
    if h != atlas_height || h % 3 != 0 {
        return Some(EditPreview { full: as_frame(rgba), planes: Vec::new() });
    }
    let height = h / 3;
    let mut full = image::RgbaImage::new(w, height);
    let mut planes = Vec::new();
    for i in 0..3 {
        let plane = image::imageops::crop_imm(&rgba, 0, height * i, w, height).to_image();
        image::imageops::overlay(&mut full, &plane, 0, 0);
        planes.push(as_frame(plane));
    }
    Some(EditPreview { full: as_frame(full), planes })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_canvas_sources_require_a_fresh_render_when_moved_back() {
        use super::super::placement::Rect;
        let inside = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        assert!(can_transform_locally(inside, 100.0, 50.0));
        for (x, y) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-200.0, 0.0)] {
            assert!(!can_transform_locally(Rect { x, y, ..inside }, 100.0, 50.0));
        }
        assert!(!can_transform_locally(Rect { w: 101.0, ..inside }, 100.0, 50.0));
        assert!(!can_transform_locally(Rect { h: 51.0, ..inside }, 100.0, 50.0));
    }
    fn png(image: image::RgbaImage) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }
    #[test]
    fn extracts_planes_and_composes_full_fallback_in_paint_order() {
        let mut atlas = image::RgbaImage::new(2, 6);
        for x in 0..2 {
            for y in 0..2 {
                atlas.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
                atlas.put_pixel(x, y + 2, image::Rgba([0, 255, 0, 255]));
            }
        }
        atlas.put_pixel(0, 4, image::Rgba([0, 0, 255, 255]));
        let preview = edit_preview(&png(atlas), 6).unwrap();
        assert_eq!(preview.planes.len(), 3);
        assert_eq!((preview.full.width, preview.full.height), (2, 2));
        assert_eq!(&preview.full.rgba[..8], &[0, 0, 255, 255, 0, 255, 0, 255]);
        assert_eq!(&preview.planes[0].rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&preview.planes[1].rgba[..4], &[0, 255, 0, 255]);
    }
    #[test]
    fn old_server_single_frame_is_a_valid_fallback_and_invalid_bytes_are_rejected() {
        let preview = edit_preview(&png(image::RgbaImage::new(2, 2)), 6).unwrap();
        assert!(preview.planes.is_empty());
        assert_eq!((preview.full.width, preview.full.height), (2, 2));
        assert!(edit_preview(b"not an image", 6).is_none());
    }
}
