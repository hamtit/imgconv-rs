//! JPEG XL encoding with libjxl 0.12.0: effort 7, whole-image buffering,
//! distance from the 0-100 quality (lossless at 100), sRGB.

#[cfg(not(target_os = "emscripten"))]
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

use crate::jxl::sys::*;

use crate::bail;
use crate::error::Result;
use crate::image::Image;

pub const DEFAULT_EFFORT: u32 = 7;
const BUFFERING_WHOLE_IMAGE: i64 = 2;

struct Encoder(*mut JxlEncoder);
impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { JxlEncoderDestroy(self.0) }
    }
}
// The wasm build has no threads (libjxl_threads is not built there).
#[cfg(not(target_os = "emscripten"))]
struct Runner(*mut c_void);
#[cfg(not(target_os = "emscripten"))]
impl Drop for Runner {
    fn drop(&mut self) {
        unsafe { JxlThreadParallelRunnerDestroy(self.0) }
    }
}

fn check(status: JxlEncoderStatus, call: &str) -> Result<()> {
    if status != JxlEncoderStatus_JXL_ENC_SUCCESS {
        bail!("{call} failed (status {status})");
    }
    Ok(())
}

#[allow(non_upper_case_globals)]
pub fn encode(img: &Image, quality: f32, effort: u32, threads: u32) -> Result<Vec<u8>> {
    let lossless = quality >= 100.0;
    unsafe {
        let enc = Encoder(JxlEncoderCreate(ptr::null()));
        if enc.0.is_null() {
            bail!("JxlEncoderCreate failed");
        }
        #[cfg(not(target_os = "emscripten"))]
        let _runner = if threads > 1 {
            let r = Runner(JxlThreadParallelRunnerCreate(ptr::null(), threads as usize));
            check(JxlEncoderSetParallelRunner(enc.0, Some(JxlThreadParallelRunner), r.0), "JxlEncoderSetParallelRunner")?;
            Some(r)
        } else {
            None
        };
        #[cfg(target_os = "emscripten")]
        let _ = threads;

        let mut info = MaybeUninit::<JxlBasicInfo>::zeroed();
        JxlEncoderInitBasicInfo(info.as_mut_ptr());
        let mut info = info.assume_init();
        info.xsize = img.width;
        info.ysize = img.height;
        info.bits_per_sample = 8;
        info.exponent_bits_per_sample = 0;
        info.num_color_channels = 3;
        if img.has_alpha() {
            info.num_extra_channels = 1;
            info.alpha_bits = 8;
            info.alpha_exponent_bits = 0;
            info.alpha_premultiplied = JXL_FALSE as i32;
        }
        info.uses_original_profile = i32::from(lossless);
        check(JxlEncoderSetBasicInfo(enc.0, &info), "JxlEncoderSetBasicInfo")?;

        let mut color = MaybeUninit::<JxlColorEncoding>::zeroed();
        JxlColorEncodingSetToSRGB(color.as_mut_ptr(), JXL_FALSE as i32);
        check(JxlEncoderSetColorEncoding(enc.0, color.as_ptr()), "JxlEncoderSetColorEncoding")?;

        let settings = JxlEncoderFrameSettingsCreate(enc.0, ptr::null());
        if settings.is_null() {
            bail!("JxlEncoderFrameSettingsCreate failed");
        }
        check(
            JxlEncoderFrameSettingsSetOption(settings, JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_EFFORT, i64::from(effort.clamp(1, 10))),
            "JxlEncoderFrameSettingsSetOption(effort)",
        )?;
        check(
            JxlEncoderFrameSettingsSetOption(settings, JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_BUFFERING, BUFFERING_WHOLE_IMAGE),
            "JxlEncoderFrameSettingsSetOption(buffering)",
        )?;
        if lossless {
            check(JxlEncoderSetFrameLossless(settings, JXL_TRUE as i32), "JxlEncoderSetFrameLossless")?;
        } else {
            check(
                JxlEncoderSetFrameDistance(settings, JxlEncoderDistanceFromQuality(quality.max(0.0))),
                "JxlEncoderSetFrameDistance",
            )?;
        }

        let format = JxlPixelFormat {
            num_channels: u32::from(img.channels),
            data_type: JxlDataType_JXL_TYPE_UINT8,
            endianness: JxlEndianness_JXL_NATIVE_ENDIAN,
            align: 0,
        };
        check(
            JxlEncoderAddImageFrame(settings, &format, img.data.as_ptr().cast(), img.data.len()),
            "JxlEncoderAddImageFrame",
        )?;
        JxlEncoderCloseInput(enc.0);

        let mut out = vec![0u8; (img.data.len() / 8).max(64 * 1024)];
        let mut used = 0usize;
        loop {
            let mut next = out.as_mut_ptr().add(used);
            let mut avail = out.len() - used;
            let status = JxlEncoderProcessOutput(enc.0, &mut next, &mut avail);
            used = out.len() - avail;
            match status {
                JxlEncoderStatus_JXL_ENC_NEED_MORE_OUTPUT => out.resize(out.len() * 2, 0),
                JxlEncoderStatus_JXL_ENC_SUCCESS => break,
                s => bail!("JxlEncoderProcessOutput failed (status {s})"),
            }
        }
        out.truncate(used);
        out.shrink_to_fit();
        Ok(out)
    }
}

pub fn version() -> String {
    let v = unsafe { JxlEncoderVersion() };
    format!("{}.{}.{}", v / 1_000_000, (v / 1000) % 1000, v % 1000)
}
