//! Saturating and exact numeric conversions.
//!
//! The workspace denies lossy `as` casts. These helpers cover the conversions
//! where clamping is the right answer: terminal geometry (a cell coordinate
//! past `u16::MAX` is off-screen anyway), display counters, and user settings
//! that are read as `u64` but used as sizes. Use `try_from` with real error
//! handling instead wherever a clamped value would be wrong data.

/// Clamp a count, width or offset to a terminal cell coordinate.
#[must_use]
pub fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Clamp a `u64` (a setting or counter) to a terminal cell coordinate.
#[must_use]
pub fn saturating_u16_from_u64(value: u64) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Clamp a count or offset to `u32`.
#[must_use]
pub fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Clamp a `u64` (a setting or counter) to `usize` on 32-bit targets.
#[must_use]
pub fn saturating_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Clamp a `u64` counter to `i64`.
#[must_use]
pub fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Clamp a count to `isize`. Counts of in-memory items never exceed
/// `isize::MAX`, so this only clamps values that cannot occur.
#[must_use]
pub fn saturating_isize(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

/// UTF-16 code units in `ch` (1 or 2), the column unit LSP positions use.
#[must_use]
pub fn utf16_units(ch: char) -> u32 {
    if ch.len_utf16() == 1 { 1 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_in_range_values_and_clamps_the_rest() {
        assert_eq!(saturating_u16(0), 0);
        assert_eq!(saturating_u16(80), 80);
        assert_eq!(saturating_u16(usize::from(u16::MAX)), u16::MAX);
        assert_eq!(saturating_u16(usize::from(u16::MAX) + 1), u16::MAX);
        assert_eq!(saturating_u16_from_u64(u64::MAX), u16::MAX);
        assert_eq!(saturating_u32(7), 7);
        assert_eq!(saturating_u32(usize::MAX), u32::MAX);
        assert_eq!(saturating_usize(42), 42);
        assert_eq!(saturating_i64(42), 42);
        assert_eq!(saturating_i64(u64::MAX), i64::MAX);
        assert_eq!(saturating_isize(3), 3);
        assert_eq!(saturating_isize(usize::MAX), isize::MAX);
    }

    #[test]
    fn counts_utf16_units() {
        assert_eq!(utf16_units('a'), 1);
        assert_eq!(utf16_units('é'), 1);
        assert_eq!(utf16_units('😀'), 2);
    }
}
