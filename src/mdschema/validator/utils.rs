use serde_json::Value;

/// Join two values together in-place.
pub fn join_values(a: &mut Value, b: Value) {
    match (a, b) {
        (Value::Object(ref mut existing_map), Value::Object(new_map)) => {
            for (key, value) in new_map {
                existing_map.insert(key, value);
            }
        }
        (Value::Array(ref mut existing_array), Value::Array(new_array)) => {
            existing_array.extend(new_array);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_values_objects() {
        let mut a = serde_json::json!({ "key1": "value1" });
        let b = serde_json::json!({ "key2": "value2" });

        join_values(&mut a, b);

        if let Value::Object(ref map) = a {
            assert_eq!(map.get("key1"), Some(&Value::String("value1".to_string())));
            assert_eq!(map.get("key2"), Some(&Value::String("value2".to_string())));
        } else {
            panic!("a is not an object");
        }
    }

    #[test]
    fn test_join_values_arrays() {
        let mut a = Value::Array(vec![Value::String("value1".to_string())]);
        let b = Value::Array(vec![
            Value::String("value2".to_string()),
            Value::String("value3".to_string()),
        ]);

        join_values(&mut a, b);

        if let Value::Array(ref array) = a {
            assert_eq!(array.len(), 3);
            assert_eq!(array[0], Value::String("value1".to_string()));
            assert_eq!(array[1], Value::String("value2".to_string()));
            assert_eq!(array[2], Value::String("value3".to_string()));
        } else {
            panic!("a is not an array");
        }
    }
}