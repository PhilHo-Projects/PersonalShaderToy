use std::{
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const SCREENSHOT_DIR: &str = "target/native-captures";
#[derive(Clone, Copy, Debug)]
struct ScreenshotReadbackLayout {
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    buffer_size: u64,
}

impl ScreenshotReadbackLayout {
    fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Cannot capture a zero-sized screenshot.".to_string());
        }

        let unpadded_bytes_per_row = width
            .checked_mul(4)
            .ok_or_else(|| "Screenshot width is too large.".to_string())?;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| "Screenshot readback buffer would be too large.".to_string())?;

        Ok(Self {
            width,
            height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            buffer_size,
        })
    }
}

pub struct ScreenshotReadback {
    buffer: wgpu::Buffer,
    layout: ScreenshotReadbackLayout,
    format: wgpu::TextureFormat,
    path: PathBuf,
}

fn texture_copy_bytes_to_rgba(
    bytes: &[u8],
    layout: &ScreenshotReadbackLayout,
    format: wgpu::TextureFormat,
) -> Result<Vec<u8>, String> {
    if bytes.len() < layout.buffer_size as usize {
        return Err("Screenshot readback returned fewer bytes than expected.".to_string());
    }

    let mut rgba = vec![0_u8; layout.unpadded_bytes_per_row as usize * layout.height as usize];
    let padded_row = layout.padded_bytes_per_row as usize;
    let unpadded_row = layout.unpadded_bytes_per_row as usize;

    for y in 0..layout.height as usize {
        let src_row = y * padded_row;
        let dst_row = y * unpadded_row;
        for x in 0..layout.width as usize {
            let src = src_row + x * 4;
            let dst = dst_row + x * 4;
            match format {
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
                    rgba[dst..dst + 4].copy_from_slice(&bytes[src..src + 4]);
                }
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                    rgba[dst] = bytes[src + 2];
                    rgba[dst + 1] = bytes[src + 1];
                    rgba[dst + 2] = bytes[src];
                    rgba[dst + 3] = bytes[src + 3];
                }
                other => {
                    return Err(format!(
                        "Screenshots do not support the current surface format: {other:?}"
                    ));
                }
            }
        }
    }

    Ok(rgba)
}

fn screenshot_output_path() -> Result<PathBuf, String> {
    let dir = Path::new(SCREENSHOT_DIR);
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create screenshot directory: {e}"))?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Ok(dir.join(format!("native-capture-{millis}.png")))
}

pub fn queue_screenshot_copy(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Result<ScreenshotReadback, String> {
    let layout = ScreenshotReadbackLayout::new(width, height)?;
    let path = screenshot_output_path()?;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot-readback"),
        size: layout.buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(layout.padded_bytes_per_row),
                rows_per_image: Some(layout.height),
            },
        },
        wgpu::Extent3d {
            width: layout.width,
            height: layout.height,
            depth_or_array_layers: 1,
        },
    );

    Ok(ScreenshotReadback {
        buffer,
        layout,
        format,
        path,
    })
}

pub fn finish_screenshot_readback(
    device: &wgpu::Device,
    readback: ScreenshotReadback,
) -> Result<PathBuf, String> {
    let ScreenshotReadback {
        buffer,
        layout,
        format,
        path,
    } = readback;
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        })
        .map_err(|e| format!("Screenshot GPU readback poll failed: {e}"))?;
    let map_result = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("Screenshot GPU readback timed out: {e}"))?;
    map_result.map_err(|e| format!("Screenshot buffer map failed: {e}"))?;

    let mapped = slice.get_mapped_range();
    let rgba_result = texture_copy_bytes_to_rgba(&mapped, &layout, format);
    drop(mapped);
    buffer.unmap();
    let rgba = rgba_result?;

    save_png(&path, layout.width, layout.height, &rgba)?;
    Ok(path)
}

fn save_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn screenshot_readback_layout_aligns_rows_to_wgpu_requirement() {
        let layout = ScreenshotReadbackLayout::new(1326, 746).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 5304);
        assert_eq!(layout.padded_bytes_per_row, 5376);
        assert_eq!(layout.buffer_size, 4_010_496);
    }

    #[test]
    fn screenshot_copy_converts_bgra_surface_bytes_to_rgba_png_bytes() {
        let layout = ScreenshotReadbackLayout::new(2, 2).unwrap();
        let mut padded = vec![0_u8; layout.buffer_size as usize];
        padded[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let row_1 = layout.padded_bytes_per_row as usize;
        padded[row_1..row_1 + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);

        let rgba =
            texture_copy_bytes_to_rgba(&padded, &layout, wgpu::TextureFormat::Bgra8UnormSrgb)
                .unwrap();

        assert_eq!(
            rgba,
            vec![3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
    }
}
