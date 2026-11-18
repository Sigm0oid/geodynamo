//! Union of S2 cells (`Covering`) converted into DynamoDB-queryable [`GeohashRange`]s.

use crate::config::GeohashEncoding;
use crate::s2::{S2CellId, S2Manager};

use super::geohash_range::GeohashRange;

/// Holds the S2 cell covering of a spatial query region.
#[derive(Debug, Clone)]
pub struct Covering {
    cell_ids: Vec<S2CellId>,
}

impl Covering {
    /// Wraps a list of covering `S2CellId`s.
    #[must_use]
    pub fn new(cell_ids: Vec<S2CellId>) -> Self {
        Self { cell_ids }
    }

    /// Number of S2 cells in this covering.
    #[must_use]
    pub fn number_of_cells(&self) -> usize {
        self.cell_ids.len()
    }

    /// Underlying covering S2 cell IDs.
    #[must_use]
    pub fn cell_ids(&self) -> &[S2CellId] {
        &self.cell_ids
    }

    /// Expands S2 cells into merged, partition-aligned [`GeohashRange`]s ready for DynamoDB LSI `BETWEEN` queries.
    #[must_use]
    pub fn get_geo_hash_ranges(
        &self,
        hash_key_length: usize,
        encoding: GeohashEncoding,
        merge_threshold: u64,
    ) -> Vec<GeohashRange> {
        let mut raw_ranges: Vec<GeohashRange> = self
            .cell_ids
            .iter()
            .map(|cell| {
                let min_id = S2Manager::encode_cell_id(cell.range_min(), encoding);
                let max_id = S2Manager::encode_cell_id(cell.range_max(), encoding);
                GeohashRange::new(min_id, max_id)
            })
            .collect();

        raw_ranges.sort_by_key(|r| r.range_min);

        let mut merged: Vec<GeohashRange> = Vec::with_capacity(raw_ranges.len());
        for range in raw_ranges {
            if let Some(last) = merged.last_mut() {
                if last.try_merge(range, merge_threshold) {
                    continue;
                }
            }
            merged.push(range);
        }

        let mut split_ranges = Vec::with_capacity(merged.len());
        for range in merged {
            split_ranges.extend(range.try_split(hash_key_length));
        }
        split_ranges
    }
}
