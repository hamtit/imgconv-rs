/// 8-bit sRGB pixels, interleaved, without row padding. `channels` is 3 (RGB)
/// or 4 (RGBA, straight / non-premultiplied alpha).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, channels: u8, data: Vec<u8>) -> Self {
        debug_assert!(channels == 3 || channels == 4);
        debug_assert_eq!(data.len(), width as usize * height as usize * channels as usize);
        Image { width, height, channels, data }
    }

    pub fn has_alpha(&self) -> bool {
        self.channels == 4
    }

    pub fn stride(&self) -> usize {
        self.width as usize * self.channels as usize
    }

    /// Drop an alpha channel that is fully opaque, so encoders can skip it.
    pub fn drop_opaque_alpha(&mut self) {
        if self.channels != 4 || !self.data.chunks_exact(4).all(|p| p[3] == 255) {
            return;
        }
        let pixels = self.width as usize * self.height as usize;
        for i in 0..pixels {
            let (s, d) = (i * 4, i * 3);
            self.data.copy_within(s..s + 3, d);
        }
        self.data.truncate(pixels * 3);
        self.data.shrink_to_fit();
        self.channels = 3;
    }

    pub fn view(&self) -> ImageView<'_> {
        ImageView { width: self.width, height: self.height, channels: self.channels, data: &self.data }
    }

    /// Return an RGBA copy (alpha 255 when the image has none).
    pub fn to_rgba(&self) -> Vec<u8> {
        if self.channels == 4 {
            return self.data.clone();
        }
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for p in self.data.chunks_exact(3) {
            out.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        out
    }
}

/// Borrowed pixels in the layout of `Image`, for passing data without copying it.
#[derive(Clone, Copy, Debug)]
pub struct ImageView<'a> {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub data: &'a [u8],
}

impl ImageView<'_> {
    pub fn has_alpha(&self) -> bool {
        self.channels == 4
    }

    pub fn stride(&self) -> usize {
        self.width as usize * self.channels as usize
    }

    /// True when the view has alpha and every alpha value is 255.
    pub fn is_opaque(&self) -> bool {
        self.channels == 4 && self.data.chunks_exact(4).all(|p| p[3] == 255)
    }

    /// RGB copy of an RGBA view, without the alpha channel.
    pub fn to_rgb(&self) -> Image {
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 3);
        for p in self.data.chunks_exact(4) {
            out.extend_from_slice(&p[..3]);
        }
        Image::new(self.width, self.height, 3, out)
    }
}

/// Expand gray / gray+alpha samples to RGB / RGBA.
pub fn expand_gray(src: &[u8], has_alpha: bool) -> Vec<u8> {
    if has_alpha {
        let mut out = Vec::with_capacity(src.len() * 2);
        for p in src.chunks_exact(2) {
            out.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
        }
        out
    } else {
        let mut out = Vec::with_capacity(src.len() * 3);
        for &g in src {
            out.extend_from_slice(&[g, g, g]);
        }
        out
    }
}
