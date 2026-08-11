use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn canonical_json_bounded<T: Serialize>(
    value: &T,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    if max_bytes == 0 {
        return Err(format!("{label} canonical JSON byte bound is invalid"));
    }
    let value = serde_json::to_value(value)
        .map_err(|error| format!("could not project {label} as canonical JSON: {error}"))?;
    let encoded = serde_json::to_vec(&sort_json(value))
        .map_err(|error| format!("could not encode {label} as canonical JSON: {error}"))?;
    if encoded.len() > max_bytes {
        return Err(format!("{label} exceeds its {max_bytes}-byte bound"));
    }
    Ok(encoded)
}

pub fn sha256_digest(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>();
            let mut object = Map::new();
            for (key, value) in sorted {
                object.insert(key, value);
            }
            Value::Object(object)
        }
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_objects_and_preserves_array_order() {
        let value = json!({"z": {"b": 2, "a": 1}, "a": [2, 1]});
        assert_eq!(
            canonical_json_bounded(&value, 1024, "test").expect("canonical JSON"),
            br#"{"a":[2,1],"z":{"a":1,"b":2}}"#
        );
        assert!(canonical_json_bounded(&value, 1, "test").is_err());
    }
}
