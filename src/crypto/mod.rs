pub mod hash;
pub mod merkle;

pub use hash::{hash_data, hash_bytes, hash_pair, hash_str};
pub use merkle::compute_merkle_root;
