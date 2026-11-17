//! An effect's images (the manifest's `images`; Cyrus, 2026-11-17: Watercolor's Pane 0 shows a
//! gallery of public-domain paintings): the files as they are read, their sizes written into the
//! shader's preamble, and one texture array the wash samples at `[[texture(0)]]`, made once when
//! the effect is put on. Every draw binds it, or a one-pixel stand-in for an effect with none, so
//! a wash that declares it never reads an unbound texture.

use metal::{DeviceRef, MTLPixelFormat, MTLRegion, MTLTextureType, MTLTextureUsage, Texture, TextureDescriptor};

/// The most images an effect may have, and the side each is stretched to in the texture array:
/// the wash undoes the stretch with the image's own size (`ARKOS_IMAGE_SIZE`), so its edges never
/// sample past it.
pub const MAX_IMAGES: usize = 8;
pub const SIDE: u32 = 512;

/// An image's width and height in pixels, from its header alone.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?.into_dimensions().map_err(|e| e.to_string())
}

/// An image decoded and stretched to `SIDE` square, as RGBA bytes.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(img.resize_exact(SIDE, SIDE, image::imageops::FilterType::Triangle).to_rgba8().into_raw())
}

/// The lines the preamble gains for an effect's images: how many there are and each one's size,
/// in the order the manifest lists them. An effect with none still gets both, so a shared helper
/// compiles.
pub fn constants(sizes: &[(u32, u32)]) -> String {
    let list = if sizes.is_empty() { "float2(0.0, 0.0)".to_string() } else { sizes.iter().map(|(w, h)| format!("float2({w}.0, {h}.0)")).collect::<Vec<_>>().join(", ") };
    format!("\nconstant int ARKOS_IMAGE_COUNT = {};\nconstant float2 ARKOS_IMAGE_SIZE[{}] = {{{list}}};\n", sizes.len(), sizes.len().max(1))
}

/// The preamble an effect's shaders are compiled with: the contract's, then its images' sizes.
pub fn preamble(images: &[std::sync::Arc<[u8]>]) -> Result<String, String> {
    let sizes = images.iter().map(|b| dimensions(b)).collect::<Result<Vec<_>, _>>()?;
    Ok(format!("{}{}", super::PREAMBLE, constants(&sizes)))
}

/// The texture array the wash samples: one `SIDE` square slice an image, mipmapped so a small
/// Pane 0 reads it smoothly; one transparent pixel for an effect with no images.
pub fn texture(device: &DeviceRef, images: &[std::sync::Arc<[u8]>]) -> Result<Texture, String> {
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::D2Array);
    desc.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    desc.set_usage(MTLTextureUsage::ShaderRead);
    if images.is_empty() {
        desc.set_width(1);
        desc.set_height(1);
        desc.set_array_length(1);
        let tex = device.new_texture(&desc);
        let clear = [0u8; 4];
        tex.replace_region_in_slice(MTLRegion::new_2d(0, 0, 1, 1), 0, 0, clear.as_ptr() as *const _, 4, 4);
        return Ok(tex);
    }
    let levels = 1 + (SIDE as f32).log2().floor() as u64;
    desc.set_width(SIDE as u64);
    desc.set_height(SIDE as u64);
    desc.set_array_length(images.len() as u64);
    desc.set_mipmap_level_count(levels);
    let tex = device.new_texture(&desc);
    for (slice, bytes) in images.iter().enumerate() {
        let rgba = decode(bytes)?;
        tex.replace_region_in_slice(MTLRegion::new_2d(0, 0, SIDE as u64, SIDE as u64), 0, slice as u64, rgba.as_ptr() as *const _, (SIDE * 4) as u64, (SIDE * SIDE * 4) as u64);
    }
    let queue = device.new_command_queue();
    let cmd = queue.new_command_buffer();
    let blit = cmd.new_blit_command_encoder();
    blit.generate_mipmaps(&tex);
    blit.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();
    Ok(tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watercolor() -> crate::effects::manifest::Effect {
        crate::effects::manifest::load("watercolor", std::path::Path::new("/nonexistent")).unwrap()
    }

    #[test]
    fn the_preamble_carries_each_image_s_size_in_the_manifest_s_order() {
        assert_eq!(constants(&[(384, 258), (258, 384)]), "\nconstant int ARKOS_IMAGE_COUNT = 2;\nconstant float2 ARKOS_IMAGE_SIZE[2] = {float2(384.0, 258.0), float2(258.0, 384.0)};\n");
        assert_eq!(constants(&[]), "\nconstant int ARKOS_IMAGE_COUNT = 0;\nconstant float2 ARKOS_IMAGE_SIZE[1] = {float2(0.0, 0.0)};\n", "none still compiles");
        let w = watercolor();
        assert_eq!(w.images.len(), w.manifest.images.len());
        let pre = preamble(&w.images).unwrap();
        assert!(pre.starts_with(crate::effects::PREAMBLE));
        assert!(pre.contains("constant int ARKOS_IMAGE_COUNT = 8;"));
        assert!(pre.contains("float2(384.0, 258.0)"), "the Great Wave first");
    }

    #[test]
    fn an_image_is_stretched_to_its_square_slice() {
        let w = watercolor();
        let rgba = decode(&w.images[0]).unwrap();
        assert_eq!(rgba.len(), (SIDE * SIDE * 4) as usize);
        assert!(decode(b"not an image").is_err());
    }

    #[test]
    fn the_texture_holds_a_slice_an_image_or_one_clear_pixel() {
        let Some(device) = metal::Device::system_default() else { return };
        let w = watercolor();
        let tex = texture(&device, &w.images).unwrap();
        assert_eq!((tex.width(), tex.height(), tex.array_length()), (SIDE as u64, SIDE as u64, 8));
        assert!(tex.mipmap_level_count() > 1);
        let none = texture(&device, &[]).unwrap();
        assert_eq!((none.width(), none.array_length()), (1, 1));
    }

    #[test]
    fn every_gallery_image_is_credited_with_its_source_and_licence() {
        let credits = include_str!("../../effects/watercolor/gallery/CREDITS.md");
        for name in &watercolor().manifest.images {
            let file = name.trim_start_matches("gallery/");
            let line = credits.lines().find(|l| l.contains(&format!("`{file}`"))).unwrap_or_else(|| panic!("{file} is not credited"));
            assert!(line.contains("CC0") && line.contains("https://"), "{line}");
        }
    }
}
