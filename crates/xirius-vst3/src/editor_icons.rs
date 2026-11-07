//! Render the exact shared SVG assets into the owner thread's GDI paint DC.
use std::{cell::RefCell, collections::HashMap};
use windows_sys::Win32::Graphics::Gdi::*;

type Key = (usize, u32, u32, u32);
thread_local! { static CACHE: RefCell<HashMap<Key, Vec<u8>>> = RefCell::default(); }

pub(crate) fn paint(dc: HDC, id: usize, x: f32, y: f32, scale: f32, ink: u32, fill: u32) {
    macro_rules! svg {
        ($file:literal) => {
            include_str!(concat!("../../xirius-ui/assets/icons/", $file, ".svg"))
        };
    }
    let svg = match id {
        1 => svg!("power"),
        3 => svg!("popout"),
        4 => svg!("pin"),
        5 => svg!("close"),
        6 => svg!("route"),
        7 => svg!("sc"),
        8 => svg!("chevron-left"),
        9 => svg!("instrument"),
        10 => svg!("ins"),
        11 => svg!("chevron-right"),
        12 => svg!("chevron-down"),
        _ => return,
    };
    let size = (16.0 * scale).round().max(1.0) as u32;
    CACHE.with_borrow_mut(|cache| {
        if cache.len() > 256 {
            cache.clear();
        }
        let pixels = cache.entry((id, size, ink, fill)).or_insert_with(|| {
            let rgb = |c: u32| ((c & 255) << 16) | (c & 0xff00) | ((c >> 16) & 255);
            let source = svg.replace("currentColor", &format!("#{:06x}", rgb(ink)));
            let Ok(tree) = resvg::usvg::Tree::from_str(&source, &Default::default()) else {
                return Vec::new();
            };
            let Some(mut map) = resvg::tiny_skia::Pixmap::new(size, size) else {
                return Vec::new();
            };
            map.fill(resvg::tiny_skia::Color::from_rgba8(
                fill as u8,
                (fill >> 8) as u8,
                (fill >> 16) as u8,
                255,
            ));
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(
                    size as f32 / tree.size().width(),
                    size as f32 / tree.size().height(),
                ),
                &mut map.as_mut(),
            );
            let mut pixels = map.take();
            for rgba in pixels.as_chunks_mut::<4>().0 {
                rgba.swap(0, 2);
            }
            pixels
        });
        if pixels.is_empty() {
            return;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: opaque, top-down BGRA pixels match the bitmap header and outlive this call.
        unsafe {
            StretchDIBits(
                dc,
                x.round() as i32,
                y.round() as i32,
                size as i32,
                size as i32,
                0,
                0,
                size as i32,
                size as i32,
                pixels.as_ptr() as _,
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
    });
}
