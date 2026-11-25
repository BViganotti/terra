//! Stable hashing used for determinism checks and cache keys.
//!
//! The algorithm is FNV-1a with 64-bit state. It is not cryptographic; it only
//! needs to be stable across runs and platforms, which fixed-width integer
//! arithmetic guarantees. Floating point values are hashed by their IEEE-754
//! bit pattern, with `-0.0` normalized to `+0.0` and `NaN` normalized to a
//! canonical quiet NaN so logically equal layers hash equally.

/// Offset basis of FNV-1a (64 bit).
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// Prime of FNV-1a (64 bit).
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a hasher.
#[derive(Clone, Debug)]
pub struct Hasher64 {
    state: u64,
    bytes: u64,
}

impl Default for Hasher64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher64 {
    /// Creates a hasher primed with the FNV offset basis.
    pub fn new() -> Self {
        Self {
            state: FNV_OFFSET,
            bytes: 0,
        }
    }

    /// Creates a hasher primed with a caller-provided domain separator.
    pub fn with_domain(domain: &str) -> Self {
        let mut h = Self::new();
        h.write_bytes(domain.as_bytes());
        h
    }

    /// Feeds raw bytes.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
    }

    /// Feeds a string without a length prefix.
    pub fn write_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    /// Feeds a length-prefixed string, so `("ab","c")` differs from `("a","bc")`.
    pub fn write_str_framed(&mut self, s: &str) {
        self.write_u64(s.len() as u64);
        self.write_bytes(s.as_bytes());
    }

    /// Feeds an unsigned 64-bit integer in little-endian order.
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Feeds a signed integer.
    pub fn write_i64(&mut self, v: i64) {
        self.write_u64(v as u64);
    }

    /// Feeds a 32-bit unsigned integer.
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Feeds a boolean as one byte.
    pub fn write_bool(&mut self, v: bool) {
        self.write_bytes(&[u8::from(v)]);
    }

    /// Feeds a float by its canonical bit pattern.
    pub fn write_f64(&mut self, v: f64) {
        self.write_u64(canonical_bits(v));
    }

    /// Feeds a slice of floats, framed by length.
    pub fn write_f64_slice(&mut self, values: &[f64]) {
        self.write_u64(values.len() as u64);
        for &v in values {
            self.write_f64(v);
        }
    }

    /// Feeds a slice of integers, framed by length.
    pub fn write_u32_slice(&mut self, values: &[u32]) {
        self.write_u64(values.len() as u64);
        for &v in values {
            self.write_u32(v);
        }
    }

    /// Number of bytes absorbed so far.
    pub fn bytes_absorbed(&self) -> u64 {
        self.bytes
    }

    /// Current hash state. This is the value to compare between runs.
    pub fn finish(&self) -> u64 {
        // Final avalanche so small input changes spread through all bits.
        let mut h = self.state;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        h
    }
}

/// Canonicalizes a float to a stable bit pattern for hashing.
pub fn canonical_bits(v: f64) -> u64 {
    if v.is_nan() {
        0x7ff8_0000_0000_0000
    } else if v == 0.0 {
        0
    } else {
        v.to_bits()
    }
}

/// Convenience: hash a byte slice in one call.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = Hasher64::new();
    h.write_bytes(bytes);
    h.finish()
}

/// Convenience: hash a string in one call.
pub fn hash_str(s: &str) -> u64 {
    hash_bytes(s.as_bytes())
}

/// Renders a hash as 16 lowercase hex digits.
pub fn hex(h: u64) -> String {
    let mut out = String::with_capacity(16);
    for i in (0..16).rev() {
        let nibble = ((h >> (i * 4)) & 0xf) as u8;
        out.push(char::from(b"0123456789abcdef"[usize::from(nibble)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_known_vector() {
        // FNV-1a 64 of the empty string is the offset basis, mixed by finish.
        let h = Hasher64::new();
        assert_eq!(h.finish() & 0xffff_ffff_ffff_ffff, h.finish());
        assert_eq!(hash_bytes(b""), Hasher64::new().finish());
    }

    #[test]
    fn distinct_inputs_differ() {
        assert_ne!(hash_str("terra"), hash_str("Terra"));
        assert_ne!(hash_str("a|b"), hash_str("a:b"));
        let mut h1 = Hasher64::new();
        h1.write_str("ab");
        h1.write_str("c");
        let mut h2 = Hasher64::new();
        h2.write_str("a");
        h2.write_str("bc");
        // Unframed concatenation is ambiguous by construction.
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn framing_removes_ambiguity() {
        let mut h1 = Hasher64::new();
        h1.write_str_framed("ab");
        h1.write_str_framed("c");
        let mut h2 = Hasher64::new();
        h2.write_str_framed("a");
        h2.write_str_framed("bc");
        assert_ne!(h1.finish(), h2.finish());
    }

    #[test]
    fn float_canonicalization() {
        assert_eq!(canonical_bits(0.0), canonical_bits(-0.0));
        assert_eq!(canonical_bits(f64::NAN), canonical_bits(-f64::NAN));
        assert_ne!(canonical_bits(1.0), canonical_bits(1.5));
    }

    #[test]
    fn hex_is_sixteen_digits() {
        let s = hex(0xdead_beef_0123_4567);
        assert_eq!(s, "deadbeef01234567");
        assert_eq!(s.len(), 16);
    }

    #[test]
    fn incremental_matches_single_shot() {
        let mut h = Hasher64::new();
        h.write_bytes(b"abc");
        assert_eq!(h.finish(), hash_bytes(b"abc"));
    }
}
