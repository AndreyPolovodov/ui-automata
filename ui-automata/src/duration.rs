use std::time::Duration;

pub fn from_str(s: &str) -> Result<Duration, &'static str> {
    if let Some(s) = s.strip_suffix("ns") {
        Ok(Duration::from_nanos(
            s.parse().map_err(|_| "Failed to parse duration")?,
        ))
    } else if let Some(s) = s.strip_suffix("us") {
        Ok(Duration::from_micros(
            s.parse().map_err(|_| "Failed to parse duration")?,
        ))
    } else if let Some(s) = s.strip_suffix("ms") {
        Ok(Duration::from_millis(
            s.parse().map_err(|_| "Failed to parse duration")?,
        ))
    } else if let Some(s) = s.strip_suffix('s') {
        Ok(Duration::from_secs(
            s.parse().map_err(|_| "Failed to parse duration")?,
        ))
    } else if let Some(s) = s.strip_suffix('m') {
        Ok(Duration::from_secs(
            s.parse::<u64>().map_err(|_| "Failed to parse duration")? * 60,
        ))
    } else if let Some(s) = s.strip_suffix('h') {
        Ok(Duration::from_secs(
            s.parse::<u64>().map_err(|_| "Failed to parse duration")? * 3600,
        ))
    } else if let Some(s) = s.strip_suffix('d') {
        Ok(Duration::from_secs(
            s.parse::<u64>().map_err(|_| "Failed to parse duration")? * 3600 * 24,
        ))
    } else {
        Err("Wrong duration suffix: try `ms`, `s`, `m`, `h`, `d`")
    }
}

/// Serde helpers.  Only `deserialize` is provided; `serialize` is intentionally
/// omitted — add it here if you ever `#[derive(Serialize)]` on a type that uses
/// these attributes.
pub mod serde {
    use super::from_str;
    use ::serde::Deserialize;
    use std::time::Duration;

    /// `#[serde(with = "duration::serde")]` — deserializes `"5s"`, `"200ms"`, etc.
    pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let s = String::deserialize(d)?;
        from_str(&s).map_err(::serde::de::Error::custom)
    }

    /// Stub serializer required by `schemars` when using `#[serde(with = "duration::serde")]`.
    pub fn serialize<S: ::serde::Serializer>(v: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}ms", v.as_millis()))
    }

    /// `#[serde(default, with = "duration::serde::option")]` — deserializes an optional duration.
    pub mod option {
        use super::*;

        pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(
            d: D,
        ) -> Result<Option<Duration>, D::Error> {
            let s = Option::<String>::deserialize(d)?;
            match s {
                Some(s) => Ok(Some(from_str(&s).map_err(::serde::de::Error::custom)?)),
                None => Ok(None),
            }
        }

        /// Stub serializer — not used at runtime, but required so that
        /// `schemars` can resolve the `with` module when deriving `JsonSchema`.
        pub fn serialize<S: ::serde::Serializer>(
            v: &Option<Duration>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            match v {
                Some(d) => s.serialize_some(&format!("{}ms", d.as_millis())),
                None => s.serialize_none(),
            }
        }
    }
}
