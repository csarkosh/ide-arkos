//! The hash the effects' shaders use, in Rust, so the app and a test can agree with a shader to
//! the bit.

/// A hash of `n` in 0 to 1, in integer arithmetic (PCG's output step) so the shader and this
/// agree to the bit: the top 24 bits, times a power of two, are exact in a float.
pub fn hash01(n: u32) -> f32 {
    let mut h = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h = ((h >> ((h >> 28) + 4)) ^ h).wrapping_mul(277_803_737);
    h ^= h >> 22;
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}
