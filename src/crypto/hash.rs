use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn hash_data<T: Serialize>(data: &T) -> Result<String> {
    let json = serde_json::to_string(data)?;
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let result = hasher.finalize();
    Ok(hex::encode(result))
}

pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn hash_str(s: &str) -> String {
    hash_bytes(s.as_bytes())
}

pub fn hash_pair(left: &str, right: &str) -> String {
    let combined = format!("{}{}", left, right);
    hash_str(&combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_str_deterministic() {
        let h1 = hash_str("hello");
        let h2 = hash_str("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_str_different() {
        let h1 = hash_str("hello");
        let h2 = hash_str("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_data_struct() {
        #[derive(Serialize)]
        struct Foo {
            a: u32,
            b: String,
        }
        let foo = Foo { a: 42, b: "bar".into() };
        let h = hash_data(&foo).unwrap();
        assert_eq!(h.len(), 64);
    }
}
