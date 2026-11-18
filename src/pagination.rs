//! Lossless base64-JSON pagination cursor encoding for DynamoDB `AttributeValue` maps and items.

use std::collections::HashMap;

use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::AttributeValue;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{GeoError, Result};
use crate::model::requests::DynamoDbItem;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum PaginationToken {
    Stream {
        range_index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclusive_start_key: Option<HashMap<String, WireAttr>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        buffer: Vec<HashMap<String, WireAttr>>,
    },
    Sorted {
        offset: usize,
    },
}

/// Lossless JSON wire representation covering every `aws_sdk_dynamodb::types::AttributeValue` variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "v")]
pub(crate) enum WireAttr {
    S(String),
    N(String),
    B(String),
    Ss(Vec<String>),
    Ns(Vec<String>),
    Bs(Vec<String>),
    M(HashMap<String, WireAttr>),
    L(Vec<WireAttr>),
    Bool(bool),
    Null(bool),
}

impl WireAttr {
    fn from_attr(attr: &AttributeValue) -> Result<Self> {
        match attr {
            AttributeValue::S(s) => Ok(Self::S(s.clone())),
            AttributeValue::N(n) => Ok(Self::N(n.clone())),
            AttributeValue::B(b) => Ok(Self::B(URL_SAFE_NO_PAD.encode(b.as_ref()))),
            AttributeValue::Ss(v) => Ok(Self::Ss(v.clone())),
            AttributeValue::Ns(v) => Ok(Self::Ns(v.clone())),
            AttributeValue::Bs(v) => Ok(Self::Bs(
                v.iter().map(|b| URL_SAFE_NO_PAD.encode(b.as_ref())).collect(),
            )),
            AttributeValue::M(map) => Ok(Self::M(map_to_wire(map)?)),
            AttributeValue::L(list) => {
                let mut out = Vec::with_capacity(list.len());
                for elem in list {
                    out.push(Self::from_attr(elem)?);
                }
                Ok(Self::L(out))
            }
            AttributeValue::Bool(b) => Ok(Self::Bool(*b)),
            AttributeValue::Null(n) => Ok(Self::Null(*n)),
            _ => Err(GeoError::InvalidPaginationToken(
                "unsupported AttributeValue variant in cursor".to_owned(),
            )),
        }
    }

    fn into_attr(self) -> Result<AttributeValue> {
        match self {
            Self::S(s) => Ok(AttributeValue::S(s)),
            Self::N(n) => Ok(AttributeValue::N(n)),
            Self::B(b64) => {
                let bytes = URL_SAFE_NO_PAD
                    .decode(b64)
                    .map_err(|e| GeoError::InvalidPaginationToken(e.to_string()))?;
                Ok(AttributeValue::B(Blob::new(bytes)))
            }
            Self::Ss(v) => Ok(AttributeValue::Ss(v)),
            Self::Ns(v) => Ok(AttributeValue::Ns(v)),
            Self::Bs(v) => {
                let mut blobs = Vec::with_capacity(v.len());
                for b64 in v {
                    let bytes = URL_SAFE_NO_PAD
                        .decode(b64)
                        .map_err(|e| GeoError::InvalidPaginationToken(e.to_string()))?;
                    blobs.push(Blob::new(bytes));
                }
                Ok(AttributeValue::Bs(blobs))
            }
            Self::M(map) => Ok(AttributeValue::M(wire_to_map(map)?)),
            Self::L(list) => {
                let mut out = Vec::with_capacity(list.len());
                for elem in list {
                    out.push(elem.into_attr()?);
                }
                Ok(AttributeValue::L(out))
            }
            Self::Bool(b) => Ok(AttributeValue::Bool(b)),
            Self::Null(n) => Ok(AttributeValue::Null(n)),
        }
    }
}

pub(crate) fn map_to_wire(map: &DynamoDbItem) -> Result<HashMap<String, WireAttr>> {
    let mut out = HashMap::with_capacity(map.len());
    for (k, v) in map {
        out.insert(k.clone(), WireAttr::from_attr(v)?);
    }
    Ok(out)
}

pub(crate) fn wire_to_map(wire: HashMap<String, WireAttr>) -> Result<DynamoDbItem> {
    let mut out = HashMap::with_capacity(wire.len());
    for (k, v) in wire {
        out.insert(k, v.into_attr()?);
    }
    Ok(out)
}

pub(crate) fn encode_token(token: &PaginationToken) -> Result<String> {
    let json = serde_json::to_vec(token)
        .map_err(|e| GeoError::InvalidPaginationToken(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

pub(crate) fn decode_token(encoded: &str) -> Result<PaginationToken> {
    let raw = URL_SAFE_NO_PAD
        .decode(encoded.trim())
        .map_err(|e| GeoError::InvalidPaginationToken(e.to_string()))?;
    serde_json::from_slice(&raw).map_err(|e| GeoError::InvalidPaginationToken(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_all_dynamodb_attribute_value_types() {
        let mut item = DynamoDbItem::new();
        item.insert("pk".into(), AttributeValue::N("12".into()));
        item.insert("sk".into(), AttributeValue::S("item-1".into()));
        item.insert("bin".into(), AttributeValue::B(Blob::new(vec![1_u8, 2, 3])));
        item.insert("tags".into(), AttributeValue::Ss(vec!["cafe".into(), "wifi".into()]));
        item.insert("flag".into(), AttributeValue::Bool(true));

        let token = PaginationToken::Stream {
            range_index: 2,
            exclusive_start_key: Some(map_to_wire(&item).unwrap()),
            buffer: vec![map_to_wire(&item).unwrap()],
        };
        let encoded = encode_token(&token).unwrap();
        let decoded = decode_token(&encoded).unwrap();
        match decoded {
            PaginationToken::Stream {
                range_index,
                exclusive_start_key,
                buffer,
            } => {
                assert_eq!(range_index, 2);
                assert_eq!(wire_to_map(exclusive_start_key.unwrap()).unwrap(), item);
                assert_eq!(wire_to_map(buffer.into_iter().next().unwrap()).unwrap(), item);
            }
            PaginationToken::Sorted { .. } => panic!("expected Stream token"),
        }
    }
}
