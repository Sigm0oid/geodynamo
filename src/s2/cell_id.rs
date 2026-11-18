//! 64-bit Google S2 hierarchical Hilbert-curve cell identifier (`S2CellId`).

use super::geometry::S2LatLng;

/// Maximum S2 cell subdivision level (leaf level = 30, ~1 cm² resolution on Earth).
pub const MAX_LEVEL: u8 = 30;

const MAX_SIZE: i32 = 1 << MAX_LEVEL;
const SWAP_MASK: usize = 0x01;
const INVERT_MASK: usize = 0x02;

/// Maps `(orientation, ij_quadrant)` -> Hilbert curve sub-position `0..4`.
const IJ_TO_POS: [[u64; 4]; 4] = [
    [0, 1, 3, 2],
    [0, 3, 1, 2],
    [2, 3, 1, 0],
    [2, 1, 3, 0],
];

/// Maps `(orientation, hilbert_pos)` -> `ij_quadrant` (`bit 1 = i`, `bit 0 = j`).
const POS_TO_IJ: [[i32; 4]; 4] = [
    [0, 1, 3, 2],
    [0, 2, 3, 1],
    [3, 2, 0, 1],
    [3, 1, 0, 2],
];

/// Orientation XOR delta for each Hilbert curve sub-position `0..4`.
const POS_TO_ORIENTATION: [usize; 4] = [SWAP_MASK, 0, 0, INVERT_MASK | SWAP_MASK];

/// A 64-bit S2 cell identifier encoding `(face, hilbert_position, level_sentinel)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct S2CellId(pub u64);

impl S2CellId {
    /// Constructs the level-30 leaf `S2CellId` containing the given `S2LatLng`.
    #[must_use]
    pub fn from_lat_lng(lat_lng: S2LatLng) -> Self {
        let (x, y, z) = lat_lng.to_unit_xyz();
        Self::from_xyz(x, y, z)
    }

    /// Constructs a level-0 root face cell (`face` in `0..6`).
    #[must_use]
    pub const fn from_face(face: u8) -> Self {
        Self(((face as u64) << 61) | (1_u64 << 60))
    }

    /// Constructs a level-30 leaf `S2CellId` from a 3D direction vector `(x, y, z)`.
    #[must_use]
    pub fn from_xyz(x: f64, y: f64, z: f64) -> Self {
        let (face, u, v) = xyz_to_face_uv(x, y, z);
        let s = uv_to_st(u);
        let t = uv_to_st(v);
        let i = st_to_ij(s);
        let j = st_to_ij(t);
        Self::from_face_ij(face, i, j)
    }

    /// Constructs a level-30 leaf `S2CellId` from cube `face` (`0..6`) and leaf `(i, j)` coordinates.
    #[must_use]
    pub fn from_face_ij(face: u8, i: i32, j: i32) -> Self {
        let mut bits = (face as usize) & SWAP_MASK;
        let mut id = (face as u64) << 61;

        for level in 1..=MAX_LEVEL {
            let shift = MAX_LEVEL - level;
            let i_bit = ((i >> shift) & 1) as usize;
            let j_bit = ((j >> shift) & 1) as usize;
            let ij = (i_bit << 1) | j_bit;
            let pos = IJ_TO_POS[bits][ij];
            id |= pos << (2 * u32::from(MAX_LEVEL - level) + 1);
            bits ^= POS_TO_ORIENTATION[pos as usize];
        }

        Self(id | 1)
    }

    /// Returns the raw unsigned 64-bit S2 cell identifier.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.0
    }

    /// Returns the two's-complement signed 64-bit S2 cell identifier (Java/JS `S2CellId.id()`).
    #[must_use]
    pub const fn signed_id(self) -> i64 {
        self.0 as i64
    }

    /// Returns the cube face (`0..6`).
    #[must_use]
    pub const fn face(self) -> u8 {
        (self.0 >> 61) as u8
    }

    /// Returns the lowest-set sentinel bit encoding this cell's hierarchy level.
    #[must_use]
    pub const fn lsb(self) -> u64 {
        self.0 & self.0.wrapping_neg()
    }

    /// Returns the hierarchy level (`0..=30`) of this cell.
    #[must_use]
    pub const fn level(self) -> u8 {
        MAX_LEVEL - ((self.0.trailing_zeros() as u8) >> 1)
    }

    /// Returns `true` if this is a level-30 leaf cell.
    #[must_use]
    pub const fn is_leaf(self) -> bool {
        (self.0 & 1) != 0
    }

    /// Returns the ancestor cell at `level` (`0 <= level <= self.level()`).
    #[must_use]
    pub const fn parent(self, level: u8) -> Self {
        let new_lsb = 1_u64 << (2 * (MAX_LEVEL - level));
        Self((self.0 & !(new_lsb - 1)) | new_lsb)
    }

    /// Returns the minimum level-30 leaf `S2CellId` contained within this cell.
    #[must_use]
    pub const fn range_min(self) -> Self {
        Self(self.0 - (self.lsb() - 1))
    }

    /// Returns the maximum level-30 leaf `S2CellId` contained within this cell.
    #[must_use]
    pub const fn range_max(self) -> Self {
        Self(self.0 + (self.lsb() - 1))
    }

    /// Returns the 4 direct child cells at `self.level() + 1`. Panics if `self.is_leaf()`.
    #[must_use]
    pub fn children(self) -> [Self; 4] {
        debug_assert!(!self.is_leaf(), "leaf S2CellId has no children");
        let new_lsb = self.lsb() >> 2;
        [
            Self(self.0 - 3 * new_lsb),
            Self(self.0 - new_lsb),
            Self(self.0 + new_lsb),
            Self(self.0 + 3 * new_lsb),
        ]
    }

    /// Decodes `(face, i_lo, j_lo, size)` in leaf-grid coordinates for geometric bounds computation.
    pub(crate) fn to_face_ij_size(self) -> (u8, i32, i32, i32) {
        let face = self.face();
        let level = self.level();
        let mut bits = (face as usize) & SWAP_MASK;
        let mut i = 0_i32;
        let mut j = 0_i32;

        for k in 1..=level {
            let shift = u32::from(MAX_LEVEL - k);
            let pos = ((self.0 >> (2 * shift + 1)) & 0x03) as usize;
            let ij = POS_TO_IJ[bits][pos];
            i |= ((ij >> 1) & 1) << shift;
            j |= (ij & 1) << shift;
            bits ^= POS_TO_ORIENTATION[pos];
        }

        let size = 1_i32 << u32::from(MAX_LEVEL - level);
        (face, i, j, size)
    }

    /// Returns the 4 corner unit vectors and center unit vector of this S2 cell on the sphere.
    pub(crate) fn vertices_and_center_xyz(self) -> ([(f64, f64, f64); 4], (f64, f64, f64)) {
        let (face, i_lo, j_lo, size) = self.to_face_ij_size();
        let i_hi = i_lo + size;
        let j_hi = j_lo + size;

        let u_lo = st_to_uv(f64::from(i_lo) / f64::from(MAX_SIZE));
        let u_hi = st_to_uv(f64::from(i_hi) / f64::from(MAX_SIZE));
        let v_lo = st_to_uv(f64::from(j_lo) / f64::from(MAX_SIZE));
        let v_hi = st_to_uv(f64::from(j_hi) / f64::from(MAX_SIZE));
        let u_mid = 0.5 * (u_lo + u_hi);
        let v_mid = 0.5 * (v_lo + v_hi);

        let corners = [
            normalize_xyz(face_uv_to_xyz(face, u_lo, v_lo)),
            normalize_xyz(face_uv_to_xyz(face, u_hi, v_lo)),
            normalize_xyz(face_uv_to_xyz(face, u_hi, v_hi)),
            normalize_xyz(face_uv_to_xyz(face, u_lo, v_hi)),
        ];
        let center = normalize_xyz(face_uv_to_xyz(face, u_mid, v_mid));
        (corners, center)
    }
}

#[inline]
fn uv_to_st(u: f64) -> f64 {
    if u >= 0.0 {
        0.5 * (1.0 + 3.0 * u).sqrt()
    } else {
        1.0 - 0.5 * (1.0 - 3.0 * u).sqrt()
    }
}

#[inline]
fn st_to_uv(s: f64) -> f64 {
    if s >= 0.5 {
        (1.0 / 3.0) * (4.0 * s * s - 1.0)
    } else {
        (1.0 / 3.0) * (1.0 - 4.0 * (1.0 - s) * (1.0 - s))
    }
}

#[inline]
fn st_to_ij(s: f64) -> i32 {
    let raw = (f64::from(MAX_SIZE) * s).floor() as i64;
    raw.clamp(0, i64::from(MAX_SIZE - 1)) as i32
}

#[inline]
fn xyz_to_face_uv(x: f64, y: f64, z: f64) -> (u8, f64, f64) {
    let ax = x.abs();
    let ay = y.abs();
    let az = z.abs();
    let face = if ax >= ay && ax >= az {
        if x >= 0.0 { 0 } else { 3 }
    } else if ay >= az {
        if y >= 0.0 { 1 } else { 4 }
    } else if z >= 0.0 {
        2
    } else {
        5
    };

    let (u, v) = match face {
        0 => (y / x, z / x),
        1 => (-x / y, z / y),
        2 => (-x / z, -y / z),
        3 => (z / x, y / x),
        4 => (z / y, -x / y),
        _ => (-y / z, -x / z),
    };
    (face, u, v)
}

#[inline]
pub(crate) fn face_uv_to_xyz(face: u8, u: f64, v: f64) -> (f64, f64, f64) {
    match face {
        0 => (1.0, u, v),
        1 => (-u, 1.0, v),
        2 => (-u, -v, 1.0),
        3 => (-1.0, -v, -u),
        4 => (v, -1.0, -u),
        _ => (v, u, -1.0),
    }
}

#[inline]
pub(crate) fn normalize_xyz((x, y, z): (f64, f64, f64)) -> (f64, f64, f64) {
    let norm = (x * x + y * y + z * z).sqrt();
    if norm == 0.0 {
        (1.0, 0.0, 0.0)
    } else {
        (x / norm, y / norm, z / norm)
    }
}
