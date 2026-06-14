use crate::crypto::{compute_merkle_root, hash_str};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotStatus {
    Open,
    Lock,
    Done,
}

impl std::fmt::Display for SlotStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotStatus::Open => write!(f, "open"),
            SlotStatus::Lock => write!(f, "lock"),
            SlotStatus::Done => write!(f, "done"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarehouseSlot {
    pub w_a: String,
    pub a_id: String,
    pub status: SlotStatus,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributorSlot {
    pub w_a: String,
    pub a_id: String,
    pub status: SlotStatus,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmallBoxTag {
    pub tag_id: String,
    pub receiver_wallet_hash: String,
    pub box_count: u32,
    pub box_hashes: Vec<String>,
    pub merkle_root: String,
    pub created_at: String,
}

impl SmallBoxTag {
    pub fn new(receiver_wallet_hash: String, box_hashes: Vec<String>) -> Self {
        let box_count = box_hashes.len() as u32;
        let merkle_root = compute_merkle_root(box_hashes.clone());
        let tag_source = format!("{}{}{}",
            receiver_wallet_hash, box_count, merkle_root
        );
        let tag_id = hash_str(&tag_source);

        SmallBoxTag {
            tag_id,
            receiver_wallet_hash,
            box_count,
            box_hashes,
            merkle_root,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckLayer {
    pub batch_id: String,
    pub warehouses: Vec<WarehouseSlot>,
    pub distributor: DistributorSlot,
    pub current_index: usize,
    pub final_hash: String,
    pub merkle_root: String,
    pub small_box_tags: Vec<SmallBoxTag>,
    pub transition_log: Vec<TransitionRecord>,
    pub manufacturer_wallet: String,
    pub created_at: String,
    pub finalized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub index: usize,
    pub agent_id_hash: String,
    pub wallet_address: String,
    pub final_hash: String,
    pub timestamp: String,
    pub vote_count: usize,
    pub total_voters: usize,
}

impl CheckLayer {
    pub fn new(
        manufacturer_wallet: String,
        warehouse_addresses: Vec<String>,
        distributor_address: String,
        final_hash: String,
        initial_agent_id: String,
    ) -> Result<Self> {
        if warehouse_addresses.is_empty() {
            return Err(anyhow!("At least one warehouse address required"));
        }

        let now = chrono::Utc::now().to_rfc3339();
        let batch_id = hash_str(&format!("{}{}",
            manufacturer_wallet, now
        ));

        let warehouses: Vec<WarehouseSlot> = warehouse_addresses
            .iter()
            .enumerate()
            .map(|(i, addr)| WarehouseSlot {
                w_a: hash_str(addr),
                a_id: if i == 0 { hash_str(&initial_agent_id) } else { String::new() },
                status: if i == 0 { SlotStatus::Open } else { SlotStatus::Lock },
                updated_at: now.clone(),
            })
            .collect();

        let distributor_slot = DistributorSlot {
            w_a: hash_str(&distributor_address),
            a_id: String::new(),
            status: SlotStatus::Lock,
            updated_at: now.clone(),
        };

        let mut all_hashes: Vec<String> = warehouses.iter().map(|w| w.w_a.clone()).collect();
        all_hashes.push(distributor_slot.w_a.clone());
        let merkle_root = compute_merkle_root(all_hashes);

        Ok(CheckLayer {
            batch_id,
            warehouses,
            distributor: distributor_slot,
            current_index: 0,
            final_hash,
            merkle_root,
            small_box_tags: Vec::new(),
            transition_log: Vec::new(),
            manufacturer_wallet: hash_str(&manufacturer_wallet),
            created_at: now,
            finalized: false,
        })
    }

    pub fn display_map(&self) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        for (i, w) in self.warehouses.iter().enumerate() {
            map.insert(
                format!("warehouse_{}", i),
                serde_json::json!({
                    "w_a": &w.w_a[..16],
                    "a_id": if w.a_id.is_empty() { "".to_string() } else { w.a_id[..16].to_string() },
                    "status": w.status.to_string(),
                }),
            );
        }
        map.insert(
            "Distributor".to_string(),
            serde_json::json!({
                "w_a": &self.distributor.w_a[..16],
                "a_id": if self.distributor.a_id.is_empty() { "".to_string() } else { self.distributor.a_id[..16].to_string() },
                "status": self.distributor.status.to_string(),
            }),
        );
        map.insert("current_index".to_string(), serde_json::json!(self.current_index));
        map.insert("finalized".to_string(), serde_json::json!(self.finalized));
        map
    }

    pub fn apply_warehouse_transition(
        &mut self,
        submitted_final_hash: &str,
        agent_id_hash: &str,
        wallet_address: &str,
        vote_count: usize,
        total_voters: usize,
    ) -> Result<()> {
        if submitted_final_hash != self.final_hash {
            return Err(anyhow!(
                "FINAL_HASH mismatch. Expected: {}..., Got: {}...",
                &self.final_hash[..16],
                &submitted_final_hash[..16.min(submitted_final_hash.len())]
            ));
        }

        let idx = self.current_index;
        if idx >= self.warehouses.len() {
            return Err(anyhow!("All warehouse slots exhausted. Use distributor finalization."));
        }

        if self.warehouses[idx].status != SlotStatus::Open {
            return Err(anyhow!("Current slot {} is not open", idx));
        }

        let now = chrono::Utc::now().to_rfc3339();

        self.transition_log.push(TransitionRecord {
            index: idx,
            agent_id_hash: agent_id_hash.to_string(),
            wallet_address: wallet_address.to_string(),
            final_hash: submitted_final_hash.to_string(),
            timestamp: now.clone(),
            vote_count,
            total_voters,
        });

        self.warehouses[idx].status = SlotStatus::Done;
        self.warehouses[idx].updated_at = now.clone();

        self.current_index += 1;
        let next_idx = self.current_index;

        if next_idx < self.warehouses.len() {
            self.warehouses[next_idx].status = SlotStatus::Open;
            self.warehouses[next_idx].a_id = agent_id_hash.to_string();
            self.warehouses[next_idx].updated_at = now;
        } else {
            self.distributor.status = SlotStatus::Open;
            self.distributor.a_id = agent_id_hash.to_string();
            self.distributor.updated_at = now;
        }

        Ok(())
    }

    pub fn apply_distributor_finalization(
        &mut self,
        submitted_final_hash: &str,
        agent_id_hash: &str,
        wallet_address: &str,
        small_box_tag: SmallBoxTag,
        vote_count: usize,
        total_voters: usize,
    ) -> Result<()> {
        if submitted_final_hash != self.final_hash {
            return Err(anyhow!("FINAL_HASH mismatch during distributor finalization"));
        }

        if self.distributor.status != SlotStatus::Open {
            return Err(anyhow!("Distributor slot is not open yet"));
        }

        let now = chrono::Utc::now().to_rfc3339();

        self.transition_log.push(TransitionRecord {
            index: self.current_index,
            agent_id_hash: agent_id_hash.to_string(),
            wallet_address: wallet_address.to_string(),
            final_hash: submitted_final_hash.to_string(),
            timestamp: now.clone(),
            vote_count,
            total_voters,
        });

        for w in self.warehouses.iter_mut() {
            w.a_id = String::new();
            w.updated_at = now.clone();
        }

        self.distributor.status = SlotStatus::Done;
        self.distributor.updated_at = now;

        self.small_box_tags.push(small_box_tag);

        Ok(())
    }

    pub fn apply_receiver_finalization(
        &mut self,
        receiver_wallet_hash: &str,
        submitted_inside_hashes: &[String],
        submitted_final_hash: &str,
        vote_count: usize,
        total_voters: usize,
    ) -> Result<bool> {
        let tag = self.small_box_tags.iter().find(|t| {
            t.receiver_wallet_hash == receiver_wallet_hash
        });

        let tag = match tag {
            Some(t) => t.clone(),
            None => return Err(anyhow!("No small box tag found for this receiver address")),
        };

        let computed_root = compute_merkle_root(submitted_inside_hashes.to_vec());
        let computed_final = hash_str(&format!("{}{}", submitted_final_hash, computed_root));

        let hashes_match = computed_root == tag.merkle_root;
        let count_match = submitted_inside_hashes.len() as u32 == tag.box_count;

        let now = chrono::Utc::now().to_rfc3339();

        if hashes_match && count_match {
            self.transition_log.push(TransitionRecord {
                index: 999,
                agent_id_hash: receiver_wallet_hash.to_string(),
                wallet_address: receiver_wallet_hash.to_string(),
                final_hash: computed_final,
                timestamp: now,
                vote_count,
                total_voters,
            });
            self.finalized = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|_| anyhow!("check_layer.json not found — run manufacturer first"))?;
        let layer: CheckLayer = serde_json::from_str(&content)?;
        Ok(layer)
    }
}
