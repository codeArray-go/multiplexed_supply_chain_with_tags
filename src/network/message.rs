use crate::consensus::vote::VoteResponse;
use crate::state::{CheckLayer, SmallBoxTag};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum NetworkMessage {
    InitCheckLayer {
        check_layer: CheckLayer,
        broadcaster_wallet: String,
    },

    TransitionRequest {
        final_hash: String,
        agent_id_hash: String,
        wallet_address: String,
        layer_index: usize,
        batch_id: String,
    },

    DistributorFinalize {
        final_hash: String,
        agent_id_hash: String,
        wallet_address: String,
        small_box_tag: SmallBoxTag,
        batch_id: String,
    },

    ReceiverSubmit {
        wallet_address_hash: String,
        inside_hashes: Vec<String>,
        final_hash: String,
        signature: String,
        batch_id: String,
    },

    Vote(VoteResponse),

    PeerAnnounce {
        address: String,
        port: u16,
        wallet_address: String,
        role: String,
    },

    StateRequest { batch_id: String },

    StateResponse { check_layer: CheckLayer },

    Ack {
        success: bool,
        message: String,
    },
}

impl NetworkMessage {
    pub fn to_framed_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let json = serde_json::to_vec(self)?;
        let len = json.len() as u32;
        let mut bytes = len.to_be_bytes().to_vec();
        bytes.extend_from_slice(&json);
        Ok(bytes)
    }

    pub fn from_framed_bytes(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < 4 {
            return Err(anyhow::anyhow!("Frame too short"));
        }
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + len {
            return Err(anyhow::anyhow!("Incomplete frame"));
        }
        let msg = serde_json::from_slice(&data[4..4 + len])?;
        Ok(msg)
    }
}
