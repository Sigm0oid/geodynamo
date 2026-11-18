//! Geohash and partition hash-key derivation utilities (`S2Manager`).

use crate::config::GeohashEncoding;
use crate::model::GeoPoint;

use super::cell_id::S2CellId;
use super::geometry::S2LatLng;

/// Stateless helper computing 64-bit S2 geohashes and truncated partition `hashKey`s.
#[derive(Debug, Default, Clone, Copy)]
pub struct S2Manager;

impl S2Manager {
    /// Computes the level-30 `S2CellId` for `point`.
    #[must_use]
    pub fn cell_id_for_point(point: GeoPoint) -> S2CellId {
        S2CellId::from_lat_lng(S2LatLng::from_degrees(point.latitude(), point.longitude()))
    }

    /// Generates the 64-bit geohash integer (as `i128` to represent both `u64` and `i64` losslessly)
    /// according to the configured [`GeohashEncoding`].
    #[must_use]
    pub fn generate_geohash(point: GeoPoint, encoding: GeohashEncoding) -> i128 {
        let cell = Self::cell_id_for_point(point);
        Self::encode_cell_id(cell, encoding)
    }

    /// Converts a raw `S2CellId` to its configured decimal integer representation (`u64` or `i64`).
    #[must_use]
    pub const fn encode_cell_id(cell: S2CellId, encoding: GeohashEncoding) -> i128 {
        match encoding {
            GeohashEncoding::Unsigned => cell.id() as i128,
            GeohashEncoding::Signed => cell.signed_id() as i128,
        }
    }

    /// Extracts the leading `hash_key_length` base-10 digits of `geohash` (preserving sign for negative `i64` geohashes)
    /// using exact integer division with zero floating-point rounding loss.
    #[must_use]
    pub fn generate_hash_key(geohash: i128, hash_key_length: usize) -> i64 {
        let hash_key_len = hash_key_length.max(1);
        let abs_val = geohash.unsigned_abs();
        let digits = decimal_digits(abs_val);
        if digits <= hash_key_len {
            return geohash as i64;
        }
        let exp = (digits - hash_key_len) as u32;
        let denominator = 10_u128.pow(exp);
        let truncated = (abs_val / denominator) as i64;
        if geohash < 0 {
            -truncated
        } else {
            truncated
        }
    }
}

#[inline]
pub(crate) fn decimal_digits(mut n: u128) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    while n > 0 {
        n /= 10;
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_key_preserves_64bit_precision_without_float_rounding() {
        assert_eq!(S2Manager::generate_hash_key(1_199_999_999_999_999_999, 2), 11);
        assert_eq!(S2Manager::generate_hash_key(-1_199_999_999_999_999_999, 2), -11);
        assert_eq!(S2Manager::generate_hash_key(9_223_372_036_854_775_807, 6), 922_337);
        assert_eq!(S2Manager::generate_hash_key(42, 5), 42);
    }

    #[test]
    fn s2_cell_id_hierarchy_invariants_hold() {
        let p = GeoPoint::new(36.879_163, 10.243_120).expect("valid point");
        let leaf = S2Manager::cell_id_for_point(p);
        assert!(leaf.is_leaf());
        assert_eq!(leaf.level(), 30);

        for level in 0..30 {
            let ancestor = leaf.parent(level);
            assert_eq!(ancestor.level(), level);
            assert!(ancestor.range_min() <= leaf);
            assert!(leaf <= ancestor.range_max());
        }
    }
}
