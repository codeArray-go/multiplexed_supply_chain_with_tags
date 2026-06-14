pub mod mock;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParcelRecord {
    pub id: String,
    pub data_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmallBoxRecord {
    pub box_id: String,
    pub content_hash: String,
    pub parent_tag_id: String,
}

#[async_trait]
pub trait Database: Send + Sync {
    async fn store(&self, key: &str, value: &str) -> Result<()>;
    async fn fetch(&self, key: &str) -> Result<Option<String>>;
    async fn verify_manufacturer(&self, govt_id_hash: &str, company_name_hash: &str) -> Result<bool>;
    async fn verify_distributor(&self, govt_id_hash: &str) -> Result<bool>;
    async fn store_parcel(&self, record: &ParcelRecord) -> Result<()>;
    async fn fetch_parcel(&self, id: &str) -> Result<Option<ParcelRecord>>;
    async fn store_small_box(&self, record: &SmallBoxRecord) -> Result<()>;
    async fn fetch_small_boxes_by_tag(&self, tag_id: &str) -> Result<Vec<SmallBoxRecord>>;
    fn name(&self) -> &'static str;
}

pub async fn connect_or_mock(verify_url: Option<&str>, storage_url: Option<&str>) -> Box<dyn Database> {
    let has_urls = verify_url.map(|u| !u.is_empty()).unwrap_or(false)
        && storage_url.map(|u| !u.is_empty()).unwrap_or(false);

    if has_urls {
        eprintln!(
            "PostgreSQL URLs found but postgres feature is disabled in this build.\nFalling back to in-memory mock database."
        );
    } else {
        eprintln!("No DB URLs configured — using in-memory mock database.");
    }

    Box::new(mock::MockDb::new_with_seed())
}
