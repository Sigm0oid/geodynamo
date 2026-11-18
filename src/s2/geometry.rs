//! Spherical coordinates (`S2LatLng`), bounding boxes (`S2LatLngRect`), and spherical caps (`S2Cap`).

use std::f64::consts::PI;

/// Mean Earth radius in meters used by `geodynamo` across Java, JS, Python, and Rust.
pub const EARTH_RADIUS_METERS: f64 = 6_367_000.0;

/// Geographic coordinate stored in radians and degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct S2LatLng {
    lat_deg: f64,
    lng_deg: f64,
}

impl S2LatLng {
    /// Creates an `S2LatLng` from decimal degrees (`latitude`, `longitude`).
    #[must_use]
    pub fn from_degrees(lat_deg: f64, lng_deg: f64) -> Self {
        Self { lat_deg, lng_deg }
    }

    /// Latitude in decimal degrees.
    #[must_use]
    pub const fn lat_degrees(self) -> f64 {
        self.lat_deg
    }

    /// Longitude in decimal degrees.
    #[must_use]
    pub const fn lng_degrees(self) -> f64 {
        self.lng_deg
    }

    /// Latitude in radians.
    #[must_use]
    pub fn lat_radians(self) -> f64 {
        self.lat_deg.to_radians()
    }

    /// Longitude in radians.
    #[must_use]
    pub fn lng_radians(self) -> f64 {
        self.lng_deg.to_radians()
    }

    /// Converts `(lat, lng)` onto the unit sphere `(x, y, z)`.
    #[must_use]
    pub fn to_unit_xyz(self) -> (f64, f64, f64) {
        let lat = self.lat_radians();
        let lng = self.lng_radians();
        let cos_lat = lat.cos();
        (cos_lat * lng.cos(), cos_lat * lng.sin(), lat.sin())
    }

    /// Converts a unit 3D vector `(x, y, z)` back to `S2LatLng`.
    #[must_use]
    pub fn from_unit_xyz((x, y, z): (f64, f64, f64)) -> Self {
        let lat_rad = z.clamp(-1.0, 1.0).asin();
        let lng_rad = y.atan2(x);
        Self::from_degrees(lat_rad.to_degrees(), lng_rad.to_degrees())
    }

    /// Great-circle central angle (in radians) between two coordinates using the Haversine formula.
    #[must_use]
    pub fn central_angle_radians(self, other: Self) -> f64 {
        let dlat = (other.lat_radians() - self.lat_radians()) * 0.5;
        let dlng = (other.lng_radians() - self.lng_radians()) * 0.5;
        let a = dlat.sin().powi(2)
            + self.lat_radians().cos() * other.lat_radians().cos() * dlng.sin().powi(2);
        2.0 * a.clamp(0.0, 1.0).sqrt().asin()
    }

    /// Surface distance in meters along the Earth sphere (`EARTH_RADIUS_METERS`).
    #[must_use]
    pub fn distance_meters(self, other: Self) -> f64 {
        self.central_angle_radians(other) * EARTH_RADIUS_METERS
    }
}

/// Spherical latitude-longitude rectangle supporting antimeridian (`±180°`) wrapping and full-sphere bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct S2LatLngRect {
    lat_lo_deg: f64,
    lat_hi_deg: f64,
    lng_lo_deg: f64,
    lng_hi_deg: f64,
    full_lng: bool,
}

impl S2LatLngRect {
    /// Creates a rectangle from minimum (southwest) and maximum (northeast) corners.
    /// If `min_point.lng_degrees() > max_point.lng_degrees()`, the longitude interval wraps across `±180°`.
    #[must_use]
    pub fn new(min_point: S2LatLng, max_point: S2LatLng) -> Self {
        let lat_lo_deg = min_point.lat_degrees().min(max_point.lat_degrees()).clamp(-90.0, 90.0);
        let lat_hi_deg = min_point.lat_degrees().max(max_point.lat_degrees()).clamp(-90.0, 90.0);
        let lng_lo_deg = normalize_lng_deg(min_point.lng_degrees());
        let lng_hi_deg = normalize_lng_deg(max_point.lng_degrees());
        let full_lng = (min_point.lng_degrees() <= -180.0 && max_point.lng_degrees() >= 180.0)
            || (max_point.lng_degrees() - min_point.lng_degrees()) >= 360.0;
        Self {
            lat_lo_deg,
            lat_hi_deg,
            lng_lo_deg,
            lng_hi_deg,
            full_lng,
        }
    }

    /// Creates the smallest latitude-longitude rectangle enclosing two points (shortest longitude arc).
    #[must_use]
    pub fn from_point_pair(p1: S2LatLng, p2: S2LatLng) -> Self {
        let lat_lo_deg = p1.lat_degrees().min(p2.lat_degrees()).clamp(-90.0, 90.0);
        let lat_hi_deg = p1.lat_degrees().max(p2.lat_degrees()).clamp(-90.0, 90.0);
        let a = normalize_lng_deg(p1.lng_degrees());
        let b = normalize_lng_deg(p2.lng_degrees());
        let (mut lo, mut hi) = if a <= b { (a, b) } else { (b, a) };
        if hi - lo > 180.0 {
            std::mem::swap(&mut lo, &mut hi);
        }
        Self {
            lat_lo_deg,
            lat_hi_deg,
            lng_lo_deg: lo,
            lng_hi_deg: hi,
            full_lng: false,
        }
    }

    /// Minimum latitude in degrees.
    #[must_use]
    pub const fn lat_lo_degrees(&self) -> f64 {
        self.lat_lo_deg
    }

    /// Maximum latitude in degrees.
    #[must_use]
    pub const fn lat_hi_degrees(&self) -> f64 {
        self.lat_hi_deg
    }

    /// West boundary longitude in degrees.
    #[must_use]
    pub const fn lng_lo_degrees(&self) -> f64 {
        self.lng_lo_deg
    }

    /// East boundary longitude in degrees.
    #[must_use]
    pub const fn lng_hi_degrees(&self) -> f64 {
        self.lng_hi_deg
    }

    /// Returns `true` if `point` lies inside (or on the boundary of) this rectangle.
    #[must_use]
    pub fn contains(&self, point: S2LatLng) -> bool {
        let lat = point.lat_degrees();
        if lat < self.lat_lo_deg || lat > self.lat_hi_deg {
            return false;
        }
        if self.full_lng {
            return true;
        }
        let lng = normalize_lng_deg(point.lng_degrees());
        self.contains_lng(lng)
    }

    #[inline]
    pub(crate) fn contains_lng(&self, lng: f64) -> bool {
        if self.full_lng {
            return true;
        }
        if self.lng_lo_deg <= self.lng_hi_deg {
            lng >= self.lng_lo_deg && lng <= self.lng_hi_deg
        } else {
            lng >= self.lng_lo_deg || lng <= self.lng_hi_deg
        }
    }

    /// Center point of the rectangle.
    #[must_use]
    pub fn center(&self) -> S2LatLng {
        let mid_lat = 0.5 * (self.lat_lo_deg + self.lat_hi_deg);
        let mid_lng = if self.full_lng {
            0.0
        } else if self.lng_lo_deg <= self.lng_hi_deg {
            0.5 * (self.lng_lo_deg + self.lng_hi_deg)
        } else {
            normalize_lng_deg(0.5 * (self.lng_lo_deg + self.lng_hi_deg + 360.0))
        };
        S2LatLng::from_degrees(mid_lat, mid_lng)
    }

    /// Returns `true` if this rectangle intersects another `S2LatLngRect`.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        if self.lat_lo_deg > other.lat_hi_deg || self.lat_hi_deg < other.lat_lo_deg {
            return false;
        }
        if self.full_lng || other.full_lng {
            return true;
        }
        self.contains_lng(other.lng_lo_deg)
            || self.contains_lng(other.lng_hi_deg)
            || other.contains_lng(self.lng_lo_deg)
            || other.contains_lng(self.lng_hi_deg)
    }
}

/// Spherical cap (circular disc on the sphere) defined by a center coordinate and angular radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct S2Cap {
    center: S2LatLng,
    axis: (f64, f64, f64),
    radius_radians: f64,
}

impl S2Cap {
    /// Creates a spherical cap around `center` with surface radius `radius_meters`.
    #[must_use]
    pub fn from_center_meters(center: S2LatLng, radius_meters: f64) -> Self {
        let radius_radians = (radius_meters / EARTH_RADIUS_METERS).clamp(0.0, PI);
        Self {
            center,
            axis: center.to_unit_xyz(),
            radius_radians,
        }
    }

    /// Returns `true` if `point` lies within this spherical cap.
    #[must_use]
    pub fn contains(&self, point: S2LatLng) -> bool {
        self.center.central_angle_radians(point) <= self.radius_radians
    }

    /// Computes a conservative `S2LatLngRect` tightly bounding this spherical cap,
    /// expanding longitude to `[-180°, 180°]` whenever either pole is included and wrapping cleanly across `±180°`.
    #[must_use]
    pub fn get_rect_bound(&self) -> S2LatLngRect {
        let dlat_deg = self.radius_radians.to_degrees();
        let lat_lo_deg = (self.center.lat_degrees() - dlat_deg).max(-90.0);
        let lat_hi_deg = (self.center.lat_degrees() + dlat_deg).min(90.0);

        // If the cap includes the North or South Pole, all longitudes are covered.
        if lat_lo_deg <= -90.0 + 1e-12 || lat_hi_deg >= 90.0 - 1e-12 {
            return S2LatLngRect {
                lat_lo_deg: if lat_lo_deg <= -90.0 + 1e-12 { -90.0 } else { lat_lo_deg },
                lat_hi_deg: if lat_hi_deg >= 90.0 - 1e-12 { 90.0 } else { lat_hi_deg },
                lng_lo_deg: -180.0,
                lng_hi_deg: 180.0,
                full_lng: true,
            };
        }

        let cos_lat = self.center.lat_radians().cos();
        let sin_r = self.radius_radians.sin();
        if sin_r >= cos_lat || self.radius_radians >= PI * 0.5 {
            return S2LatLngRect {
                lat_lo_deg,
                lat_hi_deg,
                lng_lo_deg: -180.0,
                lng_hi_deg: 180.0,
                full_lng: true,
            };
        }

        let dlng_deg = (sin_r / cos_lat).clamp(-1.0, 1.0).asin().to_degrees();
        let lng_lo_deg = normalize_lng_deg(self.center.lng_degrees() - dlng_deg);
        let lng_hi_deg = normalize_lng_deg(self.center.lng_degrees() + dlng_deg);

        S2LatLngRect {
            lat_lo_deg,
            lat_hi_deg,
            lng_lo_deg,
            lng_hi_deg,
            full_lng: false,
        }
    }

    /// Returns the unit axis vector and angular radius (radians) for cell intersection checks.
    pub(crate) const fn axis_and_radius(&self) -> ((f64, f64, f64), f64) {
        (self.axis, self.radius_radians)
    }
}

#[inline]
fn normalize_lng_deg(lng: f64) -> f64 {
    if (-180.0..=180.0).contains(&lng) {
        return lng;
    }
    let wrapped = (lng + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped == -180.0 && lng > 0.0 {
        180.0
    } else {
        wrapped
    }
}
