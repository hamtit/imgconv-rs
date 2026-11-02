//! WebP encoding with libwebp, using libvips' webpsave settings (as used by
//! sharp): preset default, method = effort (4), alpha_quality 100.

use libwebp_sys as w;

use crate::bail;
use crate::error::Result;
use crate::image::{Image, ImageView};

fn config(quality: f32, effort: i32) -> Result<w::WebPConfig> {
    let Ok(mut config) = w::WebPConfig::new_with_preset(w::WebPPreset::WEBP_PRESET_DEFAULT, quality) else {
        bail!("libwebp config version error");
    };
    config.alpha_quality = 100;
    config.method = effort.clamp(0, 6);
    if unsafe { w::WebPValidateConfig(&config) } == 0 {
        bail!("invalid WebP configuration");
    }
    Ok(config)
}

/// A WebPPicture holding imported pixels, freed on drop.
struct Picture(w::WebPPicture);

impl Picture {
    fn import(img: ImageView, use_argb: bool) -> Result<Self> {
        let Ok(mut pic) = w::WebPPicture::new() else { bail!("libwebp picture version error") };
        pic.width = img.width as i32;
        pic.height = img.height as i32;
        pic.use_argb = i32::from(use_argb);
        let stride = img.stride() as i32;
        let ok = unsafe {
            if img.has_alpha() {
                w::WebPPictureImportRGBA(&mut pic, img.data.as_ptr(), stride)
            } else {
                w::WebPPictureImportRGB(&mut pic, img.data.as_ptr(), stride)
            }
        };
        let pic = Picture(pic);
        if ok == 0 {
            bail!("WebP picture import failed");
        }
        Ok(pic)
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        unsafe { w::WebPPictureFree(&mut self.0) }
    }
}

pub fn encode(img: &Image, quality: f32, effort: i32) -> Result<Vec<u8>> {
    let config = config(quality, effort)?;
    let mut pic = Picture::import(img.view(), false)?;
    unsafe {
        let mut writer = std::mem::zeroed::<w::WebPMemoryWriter>();
        w::WebPMemoryWriterInit(&mut writer);
        pic.0.writer = Some(w::WebPMemoryWrite);
        pic.0.custom_ptr = (&mut writer as *mut w::WebPMemoryWriter).cast();
        let ok = w::WebPEncode(&config, &mut pic.0);
        let out = if ok != 0 { Ok(std::slice::from_raw_parts(writer.mem, writer.size).to_vec()) } else {
            Err(crate::error::err(format!("WebP encoding failed ({:?})", pic.0.error_code)))
        };
        w::WebPMemoryWriterClear(&mut writer);
        out
    }
}

/// Incremental animated WebP encoder (WebPAnimEncoder), looping forever.
pub struct AnimEncoder {
    enc: *mut w::WebPAnimEncoder,
    config: w::WebPConfig,
    timestamp_ms: i32,
    pub frames: u32,
}

impl AnimEncoder {
    pub fn new(width: u32, height: u32, quality: f32, effort: i32) -> Result<Self> {
        let config = config(quality, effort)?;
        unsafe {
            let mut opts = std::mem::zeroed::<w::WebPAnimEncoderOptions>();
            if w::WebPAnimEncoderOptionsInitInternal(&mut opts, w::WEBP_MUX_ABI_VERSION as i32) == 0 {
                bail!("libwebp mux version error");
            }
            opts.anim_params.loop_count = 0;
            // libvips' defaults: effectively no forced keyframes.
            opts.kmin = i32::MAX - 1;
            opts.kmax = i32::MAX;
            opts.minimize_size = 0;
            opts.allow_mixed = 0;
            let enc = w::WebPAnimEncoderNewInternal(width as i32, height as i32, &opts, w::WEBP_MUX_ABI_VERSION as i32);
            if enc.is_null() {
                bail!("WebPAnimEncoderNew failed");
            }
            Ok(AnimEncoder { enc, config, timestamp_ms: 0, frames: 0 })
        }
    }

    /// Add an RGBA frame shown for `delay_ms` (<= 10 ms means 100 ms, as in
    /// browsers and libvips).
    pub fn add(&mut self, frame: ImageView, delay_ms: u32) -> Result<()> {
        let mut pic = Picture::import(frame, true)?;
        unsafe {
            if w::WebPAnimEncoderAdd(self.enc, &mut pic.0, self.timestamp_ms, &self.config) == 0 {
                bail!("WebP animation frame failed: {}", self.error());
            }
        }
        let delay = if delay_ms <= 10 { 100 } else { delay_ms.min(i32::MAX as u32 / 2) };
        self.timestamp_ms = self.timestamp_ms.saturating_add(delay as i32);
        self.frames += 1;
        Ok(())
    }

    fn error(&self) -> String {
        unsafe {
            let e = w::WebPAnimEncoderGetError(self.enc);
            if e.is_null() { String::new() } else { std::ffi::CStr::from_ptr(e).to_string_lossy().into_owned() }
        }
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        unsafe {
            if w::WebPAnimEncoderAdd(self.enc, std::ptr::null_mut(), self.timestamp_ms, std::ptr::null()) == 0 {
                bail!("WebP animation failed: {}", self.error());
            }
            let mut data = std::mem::zeroed::<w::WebPData>();
            if w::WebPAnimEncoderAssemble(self.enc, &mut data) == 0 {
                bail!("WebP animation assembly failed: {}", self.error());
            }
            let out = std::slice::from_raw_parts(data.bytes, data.size).to_vec();
            w::WebPDataClear(&mut data);
            Ok(out)
        }
    }
}

impl Drop for AnimEncoder {
    fn drop(&mut self) {
        unsafe { w::WebPAnimEncoderDelete(self.enc) }
    }
}

pub fn version() -> String {
    let v = unsafe { w::WebPGetEncoderVersion() };
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xff, v & 0xff)
}
