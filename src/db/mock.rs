use super::{Database, ParcelRecord, SmallBoxRecord};
use anyhow::Result;
use async_trait::async_trait;
use crate::crypto::hash::hash_str;
use std::collections::HashMap;
use tokio::sync::RwLock;

pub struct MockDb {
    store: RwLock<HashMap<String, String>>,
    manufacturers: RwLock<HashMap<String, bool>>,
    distributors: RwLock<HashMap<String, bool>>,
    parcels: RwLock<HashMap<String, ParcelRecord>>,
    small_boxes: RwLock<HashMap<String, SmallBoxRecord>>,
}

impl MockDb {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
            manufacturers: RwLock::new(HashMap::new()),
            distributors: RwLock::new(HashMap::new()),
            parcels: RwLock::new(HashMap::new()),
            small_boxes: RwLock::new(HashMap::new()),
        }
    }

    pub fn new_with_seed() -> Self {
        let db = Self::new();

        let mfr_key = format!(
            "{}{}",
            hash_str("MFR001"),
            hash_str("AcmeCorp")
        );

        let mfr_key2 = format!(
            "{}{}",
            hash_str("MFR002"),
            hash_str("GlobalGoods")
        );

        let mut mfrs = HashMap::new();
        mfrs.insert(mfr_key, true);
        mfrs.insert(mfr_key2, true);

        let dist_key = hash_str("DIST001");
        let dist_key2 = hash_str("DIST002");

        let mut dists = HashMap::new();
        dists.insert(dist_key, true);
        dists.insert(dist_key2, true);

        *db.manufacturers.try_write().unwrap() = mfrs;
        *db.distributors.try_write().unwrap() = dists;

        eprintln!(
            "MockDb seeded with:\n\
             |  Manufacturers: MFR001/AcmeCorp, MFR002/GlobalGoods\n\
             |  Distributors:  DIST001, DIST002"
        );

        db
    }

    pub async fn seed_manufacturer(&self, govt_id: &str, company_name: &str) {
        let key = format!("{}{}", hash_str(govt_id), hash_str(company_name));
        self.manufacturers.write().await.insert(key, true);
    }

    pub async fn seed_distributor(&self, govt_id: &str) {
        let key = hash_str(govt_id);
        self.distributors.write().await.insert(key, true);
    }
}

#[async_trait]
impl Database for MockDb {
    async fn store(&self, key: &str, value: &str) -> Result<()> {
        self.store.write().await.insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn fetch(&self, key: &str) -> Result<Option<String>> {
        Ok(self.store.read().await.get(key).cloned())
    }

    async fn verify_manufacturer(&self, govt_id_hash: &str, company_name_hash: &str) -> Result<bool> {
        let key = format!("{}{}", govt_id_hash, company_name_hash);
        Ok(self.manufacturers.read().await.get(&key).copied().unwrap_or(false))
    }

    async fn verify_distributor(&self, govt_id_hash: &str) -> Result<bool> {
        Ok(self.distributors.read().await.get(govt_id_hash).copied().unwrap_or(false))
    }

    async fn store_parcel(&self, record: &ParcelRecord) -> Result<()> {
        self.parcels
            .write()
            .await
            .insert(record.id.clone(), record.clone());
        Ok(())
    }

    async fn fetch_parcel(&self, id: &str) -> Result<Option<ParcelRecord>> {
        Ok(self.parcels.read().await.get(id).cloned())
    }

    async fn store_small_box(&self, record: &SmallBoxRecord) -> Result<()> {
        self.small_boxes
            .write()
            .await
            .insert(record.box_id.clone(), record.clone());
        Ok(())
    }

    async fn fetch_small_boxes_by_tag(&self, tag_id: &str) -> Result<Vec<SmallBoxRecord>> {
        let guard = self.small_boxes.read().await;
        let results = guard
            .values()
            .filter(|r| r.parent_tag_id == tag_id)
            .cloned()
            .collect();
        Ok(results)
    }

    fn name(&self) -> &'static str {
        "MockDb (in-memory)"
    }
}
