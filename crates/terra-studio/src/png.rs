//! A small PNG encoder and RGB image buffer.
//!
//! The Studio renders map layers with its own software rasterizer and encodes
//! the result with this module. Only the features Terra needs are implemented:
//! 8-bit RGB, no interlacing, one image buffer. Compression uses a fixed-Huffman
//! DEFLATE stream with a bounded LZ77 search, wrapped in a zlib container with
//! an Adler-32 trailer. PNG chunks carry CRC-32 checksums.
//!
//! The encoder is deterministic: the same pixels always produce the same bytes.
//! Tests decode the output with an independent inflater included below.

use std::fmt;

/// PNG signature.
pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Maximum image dimension accepted by [`RgbImage::new`].
pub const MAX_DIMENSION: u32 = 4_096;

/// An 8-bit RGB image.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl RgbImage {
    /// Creates an image filled with a color. Dimensions are clamped to
    /// `1..=MAX_DIMENSION`.
    pub fn new(width: u32, height: u32, color: [u8; 3]) -> Self {
        let width = width.clamp(1, MAX_DIMENSION);
        let height = height.clamp(1, MAX_DIMENSION);
        let mut data = vec![0u8; width as usize * height as usize * 3];
        for px in data.chunks_exact_mut(3) {
            px.copy_from_slice(&color);
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Raw RGB bytes, row-major, no padding.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Sets a pixel, ignoring out-of-bounds coordinates.
    pub fn set(&mut self, x: i32, y: i32, color: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        self.data[i..i + 3].copy_from_slice(&color);
    }

    /// Reads a pixel, or black outside the image.
    pub fn get(&self, x: i32, y: i32) -> [u8; 3] {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return [0, 0, 0];
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// Alpha-blends a color over a pixel.
    pub fn blend(&mut self, x: i32, y: i32, color: [u8; 3], alpha: f64) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = alpha.clamp(0.0, 1.0);
        let dst = self.get(x, y);
        let mix = |s: u8, d: u8| -> u8 {
            (f64::from(s) * a + f64::from(d) * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8
        };
        self.set(
            x,
            y,
            [
                mix(color[0], dst[0]),
                mix(color[1], dst[1]),
                mix(color[2], dst[2]),
            ],
        );
    }

    /// Fills the entire image.
    pub fn fill(&mut self, color: [u8; 3]) {
        for px in self.data.chunks_exact_mut(3) {
            px.copy_from_slice(&color);
        }
    }

    /// Draws a filled axis-aligned rectangle, clipped to the image.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 3]) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x.saturating_add(w)).min(self.width as i32);
        let y1 = (y.saturating_add(h)).min(self.height as i32);
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.set(xx, yy, color);
            }
        }
    }

    /// Draws a one-pixel line using Bresenham's algorithm.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        let mut guard = 0;
        loop {
            self.set(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
            guard += 1;
            if guard > (self.width + self.height) as i32 * 4 {
                break;
            }
        }
    }

    /// Draws a thick line by stamping a disc at each step.
    pub fn line_thick(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, radius: i32, color: [u8; 3]) {
        let r = radius.clamp(0, 8);
        let stamp = |img: &mut RgbImage, x: i32, y: i32| {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r {
                        img.set(x + dx, y + dy, color);
                    }
                }
            }
        };
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        let mut guard = 0;
        loop {
            stamp(self, x, y);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
            guard += 1;
            if guard > (self.width + self.height) as i32 * 4 {
                break;
            }
        }
    }

    /// Encodes the image as a PNG file.
    pub fn to_png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.data.len() + self.height as usize);
        for row in self.data.chunks_exact(self.width as usize * 3) {
            raw.push(0); // filter: none
            raw.extend_from_slice(row);
        }
        let compressed = zlib_compress(&raw);
        let mut out = Vec::with_capacity(compressed.len() + 128);
        out.extend_from_slice(&PNG_SIGNATURE);
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.push(8); // bit depth
        ihdr.push(2); // color type: truecolor
        ihdr.push(0); // compression
        ihdr.push(0); // filter
        ihdr.push(0); // interlace
        write_chunk(&mut out, b"IHDR", &ihdr);
        write_chunk(&mut out, b"IDAT", &compressed);
        write_chunk(&mut out, b"IEND", &[]);
        out
    }
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// CRC-32 (IEEE 802.3), as used by PNG.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// Adler-32 checksum, as used by zlib.
pub fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in data.chunks(5_552) {
        for &byte in chunk {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
    }
    (b << 16) | a
}

/// zlib container parameters used by the encoder.
pub const ZLIB_HEADER: [u8; 2] = [0x78, 0x01];

/// Compresses bytes into a zlib stream with one fixed-Huffman DEFLATE block.
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::default();
    writer.write_bits(1, 1); // BFINAL
    writer.write_bits(1, 2); // BTYPE = 01, fixed Huffman
    let mut tokens = 0usize;

    const WINDOW: usize = 32_768;
    const MIN_MATCH: usize = 3;
    const MAX_MATCH: usize = 258;
    const HASH_SIZE: usize = 1 << 15;
    const MAX_CHAIN: usize = 48;

    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut prev = vec![u32::MAX; WINDOW];
    let hash = |p: usize, data: &[u8]| -> usize {
        if p + 2 >= data.len() {
            return 0;
        }
        let v = (u32::from(data[p]) << 16) | (u32::from(data[p + 1]) << 8) | u32::from(data[p + 2]);
        (v.wrapping_mul(2_654_435_761) >> 17) as usize % HASH_SIZE
    };

    let mut pos = 0usize;
    while pos < data.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if pos + MIN_MATCH <= data.len() {
            let h = hash(pos, data);
            let mut cand = head[h];
            let mut chain = 0usize;
            let max_len = (data.len() - pos).min(MAX_MATCH);
            while cand != u32::MAX && chain < MAX_CHAIN {
                let c = cand as usize;
                if pos > c && pos - c <= WINDOW {
                    let mut l = 0usize;
                    while l < max_len && data[c + l] == data[pos + l] {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        best_dist = pos - c;
                        if l == max_len {
                            break;
                        }
                    }
                }
                cand = prev[c % WINDOW];
                chain += 1;
            }
        }

        if best_len >= MIN_MATCH {
            write_length(&mut writer, best_len);
            write_distance(&mut writer, best_dist);
            // Insert every covered position into the hash chains.
            for p in pos..(pos + best_len).min(data.len()) {
                if p + MIN_MATCH <= data.len() {
                    let h = hash(p, data);
                    prev[p % WINDOW] = head[h];
                    head[h] = p as u32;
                }
            }
            pos += best_len;
        } else {
            write_literal(&mut writer, u32::from(data[pos]));
            if pos + MIN_MATCH <= data.len() {
                let h = hash(pos, data);
                prev[pos % WINDOW] = head[h];
                head[h] = pos as u32;
            }
            pos += 1;
        }
        tokens += 1;
    }
    write_literal(&mut writer, 256); // end of block
    writer.flush();
    let mut out = Vec::with_capacity(writer.bytes.len() + 6);
    out.extend_from_slice(&ZLIB_HEADER);
    out.extend_from_slice(&writer.bytes);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    let _ = tokens;
    out
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bit: u32,
    acc: u32,
}

impl BitWriter {
    fn write_bits(&mut self, value: u32, count: u32) {
        let mut v = value;
        for _ in 0..count {
            self.acc |= (v & 1) << self.bit;
            v >>= 1;
            self.bit += 1;
            if self.bit == 8 {
                self.bytes.push(self.acc as u8);
                self.acc = 0;
                self.bit = 0;
            }
        }
    }

    /// Writes a Huffman code most-significant bit first, as DEFLATE requires.
    fn write_code(&mut self, code: u32, len: u32) {
        for i in (0..len).rev() {
            self.write_bits((code >> i) & 1, 1);
        }
    }

    fn flush(&mut self) {
        if self.bit > 0 {
            self.bytes.push(self.acc as u8);
            self.acc = 0;
            self.bit = 0;
        }
    }
}

fn write_literal(w: &mut BitWriter, lit: u32) {
    match lit {
        0..=143 => w.write_code(0x30 + lit, 8),
        144..=255 => w.write_code(0x190 + (lit - 144), 9),
        256..=279 => w.write_code(lit - 256, 7),
        _ => w.write_code(0xc0 + (lit - 280), 8),
    }
}

const LENGTH_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn write_length(w: &mut BitWriter, len: usize) {
    let len = len.clamp(3, 258) as u32;
    let mut idx = 0usize;
    for (i, &base) in LENGTH_BASE.iter().enumerate() {
        if len >= base {
            idx = i;
        } else {
            break;
        }
    }
    // Code 285 is the only length-258 code.
    if len == 258 {
        write_literal(w, 285);
        return;
    }
    let sym = 257 + idx as u32;
    write_literal(w, sym);
    let extra = LENGTH_EXTRA[idx];
    if extra > 0 {
        w.write_bits(len - LENGTH_BASE[idx], extra);
    }
}

fn write_distance(w: &mut BitWriter, dist: usize) {
    let dist = dist.clamp(1, 32_768) as u32;
    let mut idx = 0usize;
    for (i, &base) in DIST_BASE.iter().enumerate() {
        if dist >= base {
            idx = i;
        } else {
            break;
        }
    }
    w.write_code(idx as u32, 5);
    let extra = DIST_EXTRA[idx];
    if extra > 0 {
        w.write_bits(dist - DIST_BASE[idx], extra);
    }
}

/// Errors produced by the test-only decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub &'static str);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// A minimal zlib decoder used by tests to prove encoder output is valid.
///
/// Supports stored and fixed-Huffman blocks, which is exactly the subset the
/// encoder emits. It is compiled only in tests and in the fuzz harness.
pub fn zlib_decompress(input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if input.len() < 6 {
        return Err(DecodeError("short zlib stream"));
    }
    if input[0] & 0x0f != 8 {
        return Err(DecodeError("not deflate"));
    }
    let mut r = BitReader {
        data: &input[2..input.len() - 4],
        pos: 0,
        bit: 0,
    };
    let mut out = Vec::new();
    loop {
        let final_block = r.bits(1)? == 1;
        let kind = r.bits(2)?;
        match kind {
            0 => {
                r.align();
                let len = r.byte()? as usize | ((r.byte()? as usize) << 8);
                let nlen = r.byte()? as usize | ((r.byte()? as usize) << 8);
                if len != (!nlen & 0xffff) {
                    return Err(DecodeError("stored length mismatch"));
                }
                for _ in 0..len {
                    out.push(r.byte()?);
                }
            }
            1 => decode_fixed(&mut r, &mut out)?,
            _ => return Err(DecodeError("unsupported deflate block")),
        }
        if final_block {
            break;
        }
    }
    let expected = u32::from_be_bytes([
        input[input.len() - 4],
        input[input.len() - 3],
        input[input.len() - 2],
        input[input.len() - 1],
    ]);
    if adler32(&out) != expected {
        return Err(DecodeError("adler mismatch"));
    }
    Ok(out)
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn bits(&mut self, count: u32) -> Result<u32, DecodeError> {
        let mut value = 0u32;
        for i in 0..count {
            if self.pos >= self.data.len() {
                return Err(DecodeError("out of input"));
            }
            let b = (self.data[self.pos] >> self.bit) & 1;
            value |= u32::from(b) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(value)
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    fn byte(&mut self) -> Result<u8, DecodeError> {
        if self.bit != 0 {
            return Err(DecodeError("unaligned byte read"));
        }
        let b = *self.data.get(self.pos).ok_or(DecodeError("out of input"))?;
        self.pos += 1;
        Ok(b)
    }
}

fn decode_fixed(r: &mut BitReader<'_>, out: &mut Vec<u8>) -> Result<(), DecodeError> {
    loop {
        // Decode one literal/length symbol from the fixed code.
        let mut code = 0u32;
        let mut len = 0u32;
        let symbol = loop {
            code = (code << 1) | r.bits(1)?;
            len += 1;
            if let Some(s) = fixed_symbol(code, len) {
                break s;
            }
            if len > 15 {
                return Err(DecodeError("bad fixed code"));
            }
        };
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            257..=285 => {
                let idx = (symbol - 257) as usize;
                if idx >= LENGTH_BASE.len() {
                    return Err(DecodeError("bad length symbol"));
                }
                let extra = LENGTH_EXTRA[idx];
                let len = LENGTH_BASE[idx] + r.bits(extra)?;
                // Fixed distance code: 5 bits, MSB first.
                let mut dcode = 0u32;
                for _ in 0..5 {
                    dcode = (dcode << 1) | r.bits(1)?;
                }
                if dcode as usize >= DIST_BASE.len() {
                    return Err(DecodeError("bad distance code"));
                }
                let dextra = DIST_EXTRA[dcode as usize];
                let dist = DIST_BASE[dcode as usize] + r.bits(dextra)?;
                if dist as usize > out.len() {
                    return Err(DecodeError("distance past start"));
                }
                let start = out.len() - dist as usize;
                for i in 0..len as usize {
                    let b = out[start + i];
                    out.push(b);
                }
            }
            _ => return Err(DecodeError("bad symbol")),
        }
    }
}

fn fixed_symbol(code: u32, len: u32) -> Option<u32> {
    match len {
        7 => {
            if code <= 0x17 {
                Some(256 + code)
            } else {
                None
            }
        }
        8 => {
            if (0x30..=0xbf).contains(&code) {
                Some(code - 0x30)
            } else if (0xc0..=0xc7).contains(&code) {
                Some(280 + code - 0xc0)
            } else {
                None
            }
        }
        9 => {
            if (0x190..=0x1ff).contains(&code) {
                Some(144 + code - 0x190)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_known_values() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"abc"), 0x024d_0127);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn zlib_round_trips_patterns() {
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"hello".to_vec(),
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(),
            (0..5000u32).map(|i| (i % 251) as u8).collect(),
            (0..3000u32).map(|i| ((i * 7) % 13) as u8).collect(),
        ];
        for data in cases {
            let compressed = zlib_compress(&data);
            assert_eq!(&compressed[..2], &ZLIB_HEADER);
            let back = zlib_decompress(&compressed)
                .unwrap_or_else(|e| panic!("decode failed for {} bytes: {e}", data.len()));
            assert_eq!(back, data);
        }
    }

    #[test]
    fn zlib_rejects_corruption() {
        let mut compressed = zlib_compress(b"some data that will be corrupted");
        let mid = compressed.len() / 2;
        compressed[mid] ^= 0xff;
        assert!(zlib_decompress(&compressed).is_err());
        assert!(zlib_decompress(b"").is_err());
        assert!(zlib_decompress(&[0x78, 0x01, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn image_bounds_are_safe() {
        let mut img = RgbImage::new(4, 3, [1, 2, 3]);
        assert_eq!(img.width(), 4);
        assert_eq!(img.height(), 3);
        assert_eq!(img.get(0, 0), [1, 2, 3]);
        assert_eq!(img.get(-1, 0), [0, 0, 0]);
        assert_eq!(img.get(4, 0), [0, 0, 0]);
        img.set(-1, -1, [9, 9, 9]);
        img.set(99, 99, [9, 9, 9]);
        img.set(0, 0, [5, 6, 7]);
        assert_eq!(img.get(0, 0), [5, 6, 7]);
        img.line(0, 0, 3, 2, [50, 60, 70]);
        img.line_thick(0, 2, 3, 0, 2, [80, 90, 100]);
        assert_eq!(img.data().len(), 4 * 3 * 3);
    }

    #[test]
    fn blend_is_bounded() {
        let mut img = RgbImage::new(2, 2, [0, 0, 0]);
        img.blend(0, 0, [255, 255, 255], 0.5);
        assert_eq!(img.get(0, 0), [128, 128, 128]);
        img.blend(0, 0, [255, 255, 255], 2.0);
        assert_eq!(img.get(0, 0), [255, 255, 255]);
    }

    #[test]
    fn fill_rect_clips() {
        let mut img = RgbImage::new(8, 8, [0, 0, 0]);
        img.fill_rect(-2, -2, 4, 4, [10, 20, 30]);
        assert_eq!(img.get(0, 0), [10, 20, 30]);
        assert_eq!(img.get(1, 1), [10, 20, 30]);
        assert_eq!(img.get(3, 3), [0, 0, 0]);
        img.fill_rect(6, 6, 100, 100, [1, 1, 1]);
        assert_eq!(img.get(7, 7), [1, 1, 1]);
    }

    #[test]
    fn dimensions_are_clamped() {
        let img = RgbImage::new(0, 99_999, [0, 0, 0]);
        assert_eq!(img.width(), 1);
        assert_eq!(img.height(), MAX_DIMENSION);
    }

    #[test]
    fn png_structure_is_valid() {
        let img = RgbImage::new(3, 2, [10, 20, 30]);
        let png = img.to_png();
        assert_eq!(&png[..8], &PNG_SIGNATURE);
        let mut pos = 8;
        let mut kinds = Vec::new();
        while pos + 12 <= png.len() {
            let len =
                u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
            let kind = &png[pos + 4..pos + 8];
            let data = &png[pos + 8..pos + 8 + len];
            let crc = u32::from_be_bytes([
                png[pos + 8 + len],
                png[pos + 9 + len],
                png[pos + 10 + len],
                png[pos + 11 + len],
            ]);
            let mut check = Vec::new();
            check.extend_from_slice(kind);
            check.extend_from_slice(data);
            assert_eq!(
                crc32(&check),
                crc,
                "bad crc in {:?}",
                String::from_utf8_lossy(kind)
            );
            kinds.push(String::from_utf8_lossy(kind).to_string());
            pos += 12 + len;
        }
        assert_eq!(pos, png.len());
        assert_eq!(kinds, vec!["IHDR", "IDAT", "IEND"]);
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 3);
        assert_eq!(u32::from_be_bytes([png[20], png[21], png[22], png[23]]), 2);
    }

    #[test]
    fn png_pixels_round_trip() {
        let mut img = RgbImage::new(5, 4, [0, 0, 0]);
        for y in 0..4 {
            for x in 0..5 {
                img.set(x, y, [(x * 10) as u8, (y * 20) as u8, ((x + y) * 7) as u8]);
            }
        }
        let png = img.to_png();
        let mut pos = 8;
        let mut idat = Vec::new();
        while pos + 12 <= png.len() {
            let len =
                u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
            let kind = &png[pos + 4..pos + 8];
            if kind == b"IDAT" {
                idat.extend_from_slice(&png[pos + 8..pos + 8 + len]);
            }
            pos += 12 + len;
        }
        let raw = zlib_decompress(&idat).unwrap();
        let stride = 5 * 3 + 1;
        assert_eq!(raw.len(), stride * 4);
        for y in 0..4 {
            assert_eq!(raw[y * stride], 0);
            for x in 0..5 {
                let i = y * stride + 1 + x * 3;
                assert_eq!(
                    [raw[i], raw[i + 1], raw[i + 2]],
                    img.get(x as i32, y as i32)
                );
            }
        }
    }

    #[test]
    fn encoding_is_deterministic() {
        let mut a = RgbImage::new(16, 16, [3, 4, 5]);
        for x in 0..16 {
            a.set(x, x, [200, 100, 50]);
        }
        assert_eq!(a.to_png(), a.to_png());
    }
}
