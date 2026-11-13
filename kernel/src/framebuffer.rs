//! Pixel-level framebuffer output.

use bootloader_api::info::{FrameBuffer, PixelFormat};
use font8x8::legacy::BASIC_LEGACY;

pub const BACKGROUND: [u8; 3] = [10, 17, 38];
pub const FOREGROUND: [u8; 3] = [96, 245, 190];

pub fn clear(framebuffer: &mut FrameBuffer, color: [u8; 3]) {
    let info = framebuffer.info();
    for y in 0..info.height {
        for x in 0..info.width {
            put_pixel(framebuffer, x, y, color);
        }
    }
}

/// Moves the visible pixels upward by one text row and clears the new bottom row.
pub fn scroll_up(framebuffer: &mut FrameBuffer, rows: usize, pixel_rows: usize, color: [u8; 3]) {
    let (width, height, stride, bytes_per_pixel) = {
        let info = framebuffer.info();
        (info.width, info.height, info.stride, info.bytes_per_pixel)
    };
    let scroll_bytes = pixel_rows * stride * bytes_per_pixel;
    let visible_bytes = height * stride * bytes_per_pixel;

    if pixel_rows >= height || scroll_bytes >= visible_bytes {
        clear(framebuffer, color);
        return;
    }

    framebuffer
        .buffer_mut()
        .copy_within(scroll_bytes..visible_bytes, 0);
    fill_rect(
        framebuffer,
        0,
        (rows - 1) * pixel_rows,
        width,
        pixel_rows,
        color,
    );
}

pub fn draw_char(
    framebuffer: &mut FrameBuffer,
    x: usize,
    y: usize,
    character: char,
    scale: usize,
    color: [u8; 3],
) {
    let glyph = BASIC_LEGACY.get(character as usize).unwrap_or(&BASIC_LEGACY[b'?' as usize]);
    for (row, bits) in glyph.iter().enumerate() {
        for column in 0..8 {
            if bits & (1 << column) != 0 {
                fill_rect(framebuffer, x + column * scale, y + row * scale, scale, scale, color);
            }
        }
    }
}

pub(crate) fn fill_rect(
    framebuffer: &mut FrameBuffer,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: [u8; 3],
) {
    let info = framebuffer.info();
    for pixel_y in y..(y + height).min(info.height) {
        for pixel_x in x..(x + width).min(info.width) {
            put_pixel(framebuffer, pixel_x, pixel_y, color);
        }
    }
}

fn put_pixel(framebuffer: &mut FrameBuffer, x: usize, y: usize, color: [u8; 3]) {
    let info = framebuffer.info();
    let offset = (y * info.stride + x) * info.bytes_per_pixel;
    let pixel = &mut framebuffer.buffer_mut()[offset..offset + info.bytes_per_pixel];

    match info.pixel_format {
        PixelFormat::Rgb => pixel[..3].copy_from_slice(&color),
        PixelFormat::Bgr => pixel[..3].copy_from_slice(&[color[2], color[1], color[0]]),
        PixelFormat::U8 => pixel[0] = color[1],
        _ => {}
    }
}
