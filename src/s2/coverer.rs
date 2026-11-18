//! Priority-queue S2 region coverer approximating a bounding rectangle or cap with S2 cells.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::cell_id::{S2CellId, MAX_LEVEL};
use super::geometry::{S2Cap, S2LatLng, S2LatLngRect};

/// Approximates geographic regions (`S2LatLngRect` or `S2Cap`) as a compact union of [`S2CellId`]s.
#[derive(Debug, Clone, Copy)]
pub struct S2RegionCoverer {
    min_level: u8,
    max_level: u8,
    max_cells: usize,
}

impl Default for S2RegionCoverer {
    fn default() -> Self {
        Self {
            min_level: 0,
            max_level: MAX_LEVEL,
            max_cells: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateCell {
    cell: S2CellId,
    /// Lower level (larger area) gets higher priority for refinement in the max-heap.
    priority: u8,
}

impl Ord for CandidateCell {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for CandidateCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl S2RegionCoverer {
    /// Creates a new `S2RegionCoverer` with default settings (`min_level = 0`, `max_level = 30`, `max_cells = 8`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of cells in the returned covering.
    #[must_use]
    pub fn with_max_cells(mut self, max_cells: usize) -> Self {
        self.max_cells = max_cells.max(1);
        self
    }

    /// Sets minimum and maximum S2 cell levels (`0..=30`).
    #[must_use]
    pub fn with_levels(mut self, min_level: u8, max_level: u8) -> Self {
        let max_l = max_level.min(MAX_LEVEL);
        let min_l = min_level.min(max_l);
        self.min_level = min_l;
        self.max_level = max_l;
        self
    }

    /// Computes a normalized, sorted `Vec<S2CellId>` covering `rect`.
    #[must_use]
    pub fn get_covering_rect(&self, rect: &S2LatLngRect) -> Vec<S2CellId> {
        self.cover_with(|cell| cell_may_intersect_rect(cell, rect), |cell| cell_contained_in_rect(cell, rect))
    }

    /// Computes a normalized, sorted `Vec<S2CellId>` covering `cap`.
    #[must_use]
    pub fn get_covering_cap(&self, cap: &S2Cap) -> Vec<S2CellId> {
        let bound = cap.get_rect_bound();
        self.cover_with(
            |cell| cell_may_intersect_cap(cell, cap, &bound),
            |cell| cell_contained_in_cap(cell, cap),
        )
    }

    fn cover_with<FInt, FCont>(&self, may_intersect: FInt, is_contained: FCont) -> Vec<S2CellId>
    where
        FInt: Fn(S2CellId) -> bool,
        FCont: Fn(S2CellId) -> bool,
    {
        let mut heap = BinaryHeap::new();
        let mut result: Vec<S2CellId> = Vec::new();

        for face in 0..6_u8 {
            let root = S2CellId::from_face(face);
            if may_intersect(root) {
                heap.push(CandidateCell {
                    cell: root,
                    priority: MAX_LEVEL - root.level(),
                });
            }
        }

        while let Some(candidate) = heap.pop() {
            let cell = candidate.cell;
            let level = cell.level();

            if level >= self.min_level
                && (level >= self.max_level
                    || is_contained(cell)
                    || result.len() + heap.len() + 4 > self.max_cells.max(4))
            {
                result.push(cell);
                continue;
            }

            if cell.is_leaf() {
                result.push(cell);
                continue;
            }

            let mut intersecting_children = Vec::with_capacity(4);
            for child in cell.children() {
                if may_intersect(child) {
                    intersecting_children.push(child);
                }
            }

            if intersecting_children.is_empty() {
                // Conservative fallback if region lies strictly inside cell interior between vertices/center
                result.push(cell);
            } else if intersecting_children.len() == 4
                && level >= self.min_level
                && is_contained(cell)
            {
                result.push(cell);
            } else {
                for child in intersecting_children {
                    heap.push(CandidateCell {
                        cell: child,
                        priority: MAX_LEVEL - child.level(),
                    });
                }
            }
        }

        normalize_cell_union(&mut result, self.min_level);
        result
    }
}

fn cell_bounding_cap_angle(corners: &[(f64, f64, f64); 4], center: (f64, f64, f64)) -> f64 {
    let mut min_dot = 1.0_f64;
    for &(vx, vy, vz) in corners {
        let dot = (center.0 * vx + center.1 * vy + center.2 * vz).clamp(-1.0, 1.0);
        if dot < min_dot {
            min_dot = dot;
        }
    }
    // Add 2% safety margin to account for curved S2 cell edges bowing outward slightly
    (min_dot.acos() * 1.02).min(std::f64::consts::PI)
}

fn cell_lat_lng_bound(cell: S2CellId) -> S2LatLngRect {
    let (corners, center) = cell.vertices_and_center_xyz();
    let cap_radius_rad = cell_bounding_cap_angle(&corners, center);
    let center_ll = S2LatLng::from_unit_xyz(center);
    S2Cap::from_center_meters(center_ll, cap_radius_rad * super::geometry::EARTH_RADIUS_METERS)
        .get_rect_bound()
}

fn cell_may_intersect_rect(cell: S2CellId, rect: &S2LatLngRect) -> bool {
    let (corners, center) = cell.vertices_and_center_xyz();
    if rect.contains(S2LatLng::from_unit_xyz(center)) {
        return true;
    }
    for &v in &corners {
        if rect.contains(S2LatLng::from_unit_xyz(v)) {
            return true;
        }
    }
    let cell_bound = cell_lat_lng_bound(cell);
    cell_bound.intersects(rect)
}

fn cell_contained_in_rect(cell: S2CellId, rect: &S2LatLngRect) -> bool {
    if cell.level() < 2 {
        return false;
    }
    let (corners, center) = cell.vertices_and_center_xyz();
    if !rect.contains(S2LatLng::from_unit_xyz(center)) {
        return false;
    }
    corners
        .iter()
        .all(|&v| rect.contains(S2LatLng::from_unit_xyz(v)))
}

fn cell_may_intersect_cap(cell: S2CellId, cap: &S2Cap, cap_rect: &S2LatLngRect) -> bool {
    let (corners, center) = cell.vertices_and_center_xyz();
    let cell_radius = cell_bounding_cap_angle(&corners, center);
    let (axis, cap_radius) = cap.axis_and_radius();
    let dot = (center.0 * axis.0 + center.1 * axis.1 + center.2 * axis.2).clamp(-1.0, 1.0);
    let center_angle = dot.acos();
    if center_angle > cap_radius + cell_radius {
        return false;
    }
    cell_may_intersect_rect(cell, cap_rect)
}

fn cell_contained_in_cap(cell: S2CellId, cap: &S2Cap) -> bool {
    let (corners, center) = cell.vertices_and_center_xyz();
    if !cap.contains(S2LatLng::from_unit_xyz(center)) {
        return false;
    }
    corners
        .iter()
        .all(|&v| cap.contains(S2LatLng::from_unit_xyz(v)))
}

fn normalize_cell_union(cells: &mut Vec<S2CellId>, min_level: u8) {
    if cells.is_empty() {
        return;
    }
    cells.sort_by_key(|c| c.range_min());

    // Remove duplicates or cells already contained within a preceding ancestor cell
    let mut deduped: Vec<S2CellId> = Vec::with_capacity(cells.len());
    for &cell in cells.iter() {
        if let Some(&last) = deduped.last() {
            if last.range_min() <= cell.range_min() && last.range_max() >= cell.range_max() {
                continue;
            }
        }
        deduped.push(cell);
    }

    // Coalesce 4 sibling cells into their parent when above min_level
    let mut coalesced: Vec<S2CellId> = Vec::with_capacity(deduped.len());
    for cell in deduped {
        coalesced.push(cell);
        while coalesced.len() >= 4 {
            let n = coalesced.len();
            let c0 = coalesced[n - 4];
            let level = c0.level();
            if level <= min_level || level == 0 {
                break;
            }
            let parent = c0.parent(level - 1);
            if parent.children() == [coalesced[n - 4], coalesced[n - 3], coalesced[n - 2], coalesced[n - 1]] {
                coalesced.truncate(n - 4);
                coalesced.push(parent);
            } else {
                break;
            }
        }
    }

    *cells = coalesced;
}
