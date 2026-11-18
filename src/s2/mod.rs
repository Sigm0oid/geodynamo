//! Pure-Rust Google S2 Geometry primitives (`S2CellId`, `S2LatLng`, `S2LatLngRect`, `S2Cap`, and `S2RegionCoverer`).

mod cell_id;
mod coverer;
mod geometry;
mod s2_manager;

pub use cell_id::{S2CellId, MAX_LEVEL};
pub use coverer::S2RegionCoverer;
pub use geometry::{S2Cap, S2LatLng, S2LatLngRect, EARTH_RADIUS_METERS};
pub use s2_manager::S2Manager;
