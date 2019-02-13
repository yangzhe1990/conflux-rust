use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Represents rpc api epoch number param.
#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub enum EpochNumber {
    /// Number
    Num(u64),
    /// Latest epoch
    Latest,
    /// Earliest epoch (genesis)
    Earliest,
    //    Pending epoch (being mined)
    //    Pending,
}

impl Default for EpochNumber {
    fn default() -> Self {
        EpochNumber::Latest
    }
}

impl<'a> Deserialize<'a> for EpochNumber {
    fn deserialize<D>(deserializer: D) -> Result<EpochNumber, D::Error>
    where
        D: Deserializer<'a>,
    {
        deserializer.deserialize_any(EpochNumberVisitor)
    }
}

impl Serialize for EpochNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match *self {
            EpochNumber::Num(ref x) => {
                serializer.serialize_str(&format!("0x{:x}", x))
            }
            EpochNumber::Latest => serializer.serialize_str("latest"),
            EpochNumber::Earliest => serializer.serialize_str("earliest"),
            //            EpochNumber::Pending => serializer.serialize_str("pending"),
        }
    }
}

struct EpochNumberVisitor;

impl<'a> Visitor<'a> for EpochNumberVisitor {
    type Value = EpochNumber;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "a epoch number or 'latest', 'earliest' or 'pending'"
        )
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        match value {
            "latest" => Ok(EpochNumber::Latest),
            "earliest" => Ok(EpochNumber::Earliest),
            //            "pending" => Ok(EpochNumber::Pending),
            _ if value.starts_with("0x") => {
                u64::from_str_radix(&value[2..], 16)
                    .map(EpochNumber::Num)
                    .map_err(|e| {
                        Error::custom(format!("Invalid epoch number: {}", e))
                    })
            }
            _ => Err(Error::custom(format!(
                "Invalid epoch number: missing 0x prefix"
            ))),
        }
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(value.as_ref())
    }
}
