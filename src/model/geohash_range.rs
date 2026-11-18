//! Contiguous 64-bit geohash interval (`GeohashRange`) with partition-boundary splitting and range merging.

use crate::s2::S2Manager;

/// Inclusive `[range_min, range_max]` interval of 64-bit geohash integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeohashRange {
    /// Minimum geohash value (inclusive).
    pub range_min: i128,
    /// Maximum geohash value (inclusive).
    pub range_max: i128,
}

impl GeohashRange {
    /// Creates an ordered inclusive `[min(a, b), max(a, b)]` geohash range.
    #[must_use]
    pub fn new(a: i128, b: i128) -> Self {
        Self {
            range_min: a.min(b),
            range_max: a.max(b),
        }
    }

    /// Returns the partition `hashKey` corresponding to this range (assuming it has already been split).
    #[must_use]
    pub fn hash_key(&self, hash_key_length: usize) -> i64 {
        S2Manager::generate_hash_key(self.range_min, hash_key_length)
    }

    /// Attempts to merge `other` into `self` if the gap between them is at most `merge_threshold`
    /// and both endpoints share the same partition `hashKey` would remain contiguous.
    pub fn try_merge(&mut self, other: Self, merge_threshold: u64) -> bool {
        let threshold = i128::from(merge_threshold);
        if other.range_min > self.range_max && other.range_min - self.range_max <= threshold {
            self.range_max = self.range_max.max(other.range_max);
            return true;
        }
        if self.range_min > other.range_max && self.range_min - other.range_max <= threshold {
            self.range_min = self.range_min.min(other.range_min);
            return true;
        }
        // Overlapping or nested ranges
        if other.range_min <= self.range_max && other.range_max >= self.range_min {
            self.range_min = self.range_min.min(other.range_min);
            self.range_max = self.range_max.max(other.range_max);
            return true;
        }
        false
    }

    /// Splits this range into sub-ranges such that every value in each returned `GeohashRange`
    /// shares an identical `hashKey` of length `hash_key_length` (handling sign and power-of-10 boundaries).
    #[must_use]
    pub fn try_split(&self, hash_key_length: usize) -> Vec<Self> {
        let hk_len = hash_key_length.max(1);
        let mut uniform_segments = Vec::new();
        split_by_sign_and_digits(self.range_min, self.range_max, &mut uniform_segments);

        let mut output = Vec::new();
        for (lo, hi) in uniform_segments {
            let digits = decimal_digits_i128(lo);
            if digits <= hk_len {
                for v in lo..=hi {
                    output.push(Self::new(v, v));
                }
                continue;
            }

            let denom = 10_i128.pow((digits - hk_len) as u32);
            let min_hk = i128::from(S2Manager::generate_hash_key(lo, hk_len));
            let max_hk = i128::from(S2Manager::generate_hash_key(hi, hk_len));

            if min_hk == max_hk {
                output.push(Self::new(lo, hi));
            } else {
                for hk in min_hk..=max_hk {
                    let (bucket_lo, bucket_hi) = if hk >= 0 {
                        (hk * denom, (hk + 1) * denom - 1)
                    } else {
                        ((hk - 1) * denom + 1, hk * denom)
                    };
                    let clamped_lo = lo.max(bucket_lo);
                    let clamped_hi = hi.min(bucket_hi);
                    if clamped_lo <= clamped_hi {
                        output.push(Self::new(clamped_lo, clamped_hi));
                    }
                }
            }
        }
        output
    }
}

fn split_by_sign_and_digits(lo: i128, hi: i128, out: &mut Vec<(i128, i128)>) {
    if lo < 0 && hi >= 0 {
        split_negative_by_digits(lo, -1, out);
        split_positive_by_digits(0, hi, out);
    } else if hi < 0 {
        split_negative_by_digits(lo, hi, out);
    } else {
        split_positive_by_digits(lo, hi, out);
    }
}

fn split_positive_by_digits(mut cur: i128, hi: i128, out: &mut Vec<(i128, i128)>) {
    while cur <= hi {
        let d = decimal_digits_i128(cur);
        let max_with_d_digits = 10_i128.pow(d as u32) - 1;
        let seg_hi = hi.min(max_with_d_digits);
        out.push((cur, seg_hi));
        cur = seg_hi + 1;
    }
}

fn split_negative_by_digits(lo: i128, hi: i128, out: &mut Vec<(i128, i128)>) {
    // Negative range [lo, hi] where lo <= hi < 0 (e.g. [-1050, -95])
    let mut cur = lo;
    while cur <= hi {
        let d = decimal_digits_i128(cur);
        let min_abs_with_d_digits = if d == 1 { 1 } else { 10_i128.pow((d - 1) as u32) };
        let seg_hi = hi.min(-min_abs_with_d_digits);
        out.push((cur, seg_hi));
        cur = seg_hi + 1;
    }
}

fn decimal_digits_i128(v: i128) -> usize {
    let mut abs = v.unsigned_abs();
    if abs == 0 {
        return 1;
    }
    let mut d = 0;
    while abs > 0 {
        abs /= 10;
        d += 1;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_positive_and_negative_ranges_cleanly_by_hash_key() {
        let r = GeohashRange::new(1_150_000_000_000_000_000, 1_320_000_000_000_000_000);
        let parts = r.try_split(2);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], GeohashRange::new(1_150_000_000_000_000_000, 1_199_999_999_999_999_999));
        assert_eq!(parts[1], GeohashRange::new(1_200_000_000_000_000_000, 1_299_999_999_999_999_999));
        assert_eq!(parts[2], GeohashRange::new(1_300_000_000_000_000_000, 1_320_000_000_000_000_000));

        let neg = GeohashRange::new(-1_250, -1_050);
        let neg_parts = neg.try_split(2);
        assert_eq!(neg_parts.len(), 3);
        assert_eq!(neg_parts[0], GeohashRange::new(-1_250, -1_200));
        assert_eq!(neg_parts[1], GeohashRange::new(-1_199, -1_100));
        assert_eq!(neg_parts[2], GeohashRange::new(-1_099, -1_050));
    }

    #[test]
    fn splits_across_digit_length_boundary() {
        let cross = GeohashRange::new(980, 1_020);
        let parts = cross.try_split(2);
        for p in &parts {
            let hk_lo = S2Manager::generate_hash_key(p.range_min, 2);
            let hk_hi = S2Manager::generate_hash_key(p.range_max, 2);
            assert_eq!(hk_lo, hk_hi);
        }
        assert_eq!(parts.first().unwrap().range_min, 980);
        assert_eq!(parts.last().unwrap().range_max, 1_020);
    }
}
