//! Validated geographic coordinate (`GeoPoint`) with CSV and GeoJSON serialization.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::CoordinateFormat;
use crate::error::{GeoError, Result};
use crate::s2::S2LatLng;

/// A validated `(latitude, longitude)` point on Earth in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    latitude: f64,
    longitude: f64,
}

#[derive(Serialize, Deserialize)]
struct GeoJsonPointWire<'a> {
    #[serde(rename = "type")]
    point_type: &'a str,
    coordinates: [f64; 2],
}

#[derive(Deserialize)]
struct GeoJsonPointOwned {
    #[serde(rename = "type")]
    point_type: String,
    coordinates: [f64; 2],
}

impl GeoPoint {
    /// Creates a validated `GeoPoint` (`latitude` in `[-90.0, 90.0]`, `longitude` in `[-180.0, 180.0]`).
    pub fn new(latitude: f64, longitude: f64) -> Result<Self> {
        if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
            return Err(GeoError::InvalidCoordinates {
                latitude,
                longitude,
                reason: "latitude must be finite and within [-90.0, 90.0]",
            });
        }
        if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
            return Err(GeoError::InvalidCoordinates {
                latitude,
                longitude,
                reason: "longitude must be finite and within [-180.0, 180.0]",
            });
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }

    /// Latitude in decimal degrees (`[-90.0, 90.0]`).
    #[must_use]
    pub const fn latitude(self) -> f64 {
        self.latitude
    }

    /// Longitude in decimal degrees (`[-180.0, 180.0]`).
    #[must_use]
    pub const fn longitude(self) -> f64 {
        self.longitude
    }

    /// Converts this point to an internal `S2LatLng`.
    #[must_use]
    pub fn to_s2_lat_lng(self) -> S2LatLng {
        S2LatLng::from_degrees(self.latitude, self.longitude)
    }

    /// Computes surface distance in meters to `other` along the Earth sphere.
    #[must_use]
    pub fn distance_meters(self, other: Self) -> f64 {
        self.to_s2_lat_lng().distance_meters(other.to_s2_lat_lng())
    }

    /// Formats this point for storage in DynamoDB according to `format`.
    #[must_use]
    pub fn format_attr(self, format: CoordinateFormat) -> String {
        match format {
            CoordinateFormat::LatLngCsv => format!("{},{}", self.latitude, self.longitude),
            CoordinateFormat::GeoJsonPoint => {
                let wire = GeoJsonPointWire {
                    point_type: "Point",
                    coordinates: [self.longitude, self.latitude],
                };
                serde_json::to_string(&wire)
                    .unwrap_or_else(|_| format!("{},{}", self.latitude, self.longitude))
            }
        }
    }

    /// Parses a stored coordinate attribute in either `"<latitude>,<longitude>"` CSV format
    /// or `{"type":"Point","coordinates":[longitude,latitude]}` GeoJSON format.
    pub fn parse_attr(raw: &str, attribute_name: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.starts_with('{') {
            let parsed: GeoJsonPointOwned =
                serde_json::from_str(trimmed).map_err(|err| GeoError::MalformedGeoAttribute {
                    attribute: attribute_name.to_owned(),
                    details: err.to_string(),
                })?;
            if !parsed.point_type.eq_ignore_ascii_case("Point") {
                return Err(GeoError::MalformedGeoAttribute {
                    attribute: attribute_name.to_owned(),
                    details: format!("unsupported GeoJSON geometry type `{}`", parsed.point_type),
                });
            }
            return Self::new(parsed.coordinates[1], parsed.coordinates[0]);
        }

        let mut parts = trimmed.split(',');
        let lat_str = parts.next().ok_or_else(|| GeoError::MalformedGeoAttribute {
            attribute: attribute_name.to_owned(),
            details: "expected `<lat>,<lng>`".to_owned(),
        })?;
        let lng_str = parts.next().ok_or_else(|| GeoError::MalformedGeoAttribute {
            attribute: attribute_name.to_owned(),
            details: "missing longitude in `<lat>,<lng>`".to_owned(),
        })?;
        if parts.next().is_some() {
            return Err(GeoError::MalformedGeoAttribute {
                attribute: attribute_name.to_owned(),
                details: "unexpected extra comma in coordinate string".to_owned(),
            });
        }
        let lat: f64 = lat_str
            .trim()
            .parse()
            .map_err(|err: std::num::ParseFloatError| GeoError::MalformedGeoAttribute {
                attribute: attribute_name.to_owned(),
                details: err.to_string(),
            })?;
        let lng: f64 = lng_str
            .trim()
            .parse()
            .map_err(|err: std::num::ParseFloatError| GeoError::MalformedGeoAttribute {
                attribute: attribute_name.to_owned(),
                details: err.to_string(),
            })?;
        Self::new(lat, lng)
    }
}

impl fmt::Display for GeoPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.latitude, self.longitude)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_coordinate_bounds() {
        assert!(GeoPoint::new(36.879_163, 10.243_120).is_ok());
        assert!(GeoPoint::new(-90.0, 180.0).is_ok());
        assert!(GeoPoint::new(90.001, 0.0).is_err());
        assert!(GeoPoint::new(0.0, -180.1).is_err());
        assert!(GeoPoint::new(f64::NAN, 0.0).is_err());
    }

    #[test]
    fn roundtrips_csv_and_geojson_formats() {
        let p = GeoPoint::new(36.879_163, 10.243_120).unwrap();
        let csv = p.format_attr(CoordinateFormat::LatLngCsv);
        assert_eq!(GeoPoint::parse_attr(&csv, "geoJson").unwrap(), p);

        let gj = p.format_attr(CoordinateFormat::GeoJsonPoint);
        assert_eq!(GeoPoint::parse_attr(&gj, "geoJson").unwrap(), p);
    }
}
