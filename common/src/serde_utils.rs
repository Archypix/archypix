/// Generic serde helpers for common wire-format patterns.

/// Serialize / deserialize a `Vec<T>` as a single comma-separated string.
///
/// Use with `#[serde(with = "archypix_common::serde_utils::csv")]`.
///
/// Requirements on `T`:
/// - `T: std::fmt::Display`   — for serialization (`to_string()`)
/// - `T: std::str::FromStr`   — for deserialization (`parse()`)
///
/// Unknown tokens (those whose `FromStr` fails) are silently skipped during
/// deserialization, which is the right behaviour for open-ended enum filters
/// (a client sending an unrecognised job type should not cause an error).
///
/// # Wire examples
/// | Rust value                      | Query / JSON string         |
/// |---------------------------------|-----------------------------|
/// | `vec![]`                        | field omitted (use `skip_serializing_if = "Vec::is_empty"`) |
/// | `vec![GenThumbnail]`            | `"gen_thumbnail"`           |
/// | `vec![GenThumbnail, EditPicture]`| `"gen_thumbnail,edit_picture"` |
pub mod csv {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::fmt::Display;
    use std::str::FromStr;

    pub fn serialize<S, T>(values: &[T], ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Display,
    {
        ser.serialize_str(
            &values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )
    }

    pub fn deserialize<'de, D, T>(de: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: FromStr,
        T::Err: Display,
    {
        let opt = Option::<String>::deserialize(de)?;
        Ok(opt
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter_map(|s| s.trim().parse::<T>().ok())
            .collect())
    }
}
