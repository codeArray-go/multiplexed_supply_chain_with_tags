use crate::consensus::vote::VoteResponse;
use crate::crypto::hash::hash_str;
use crate::db::Database;
use crate::network::message::NetworkMessage;
use crate::network::peer::{read_message, write_message};
use crate::state::CheckLayer;
use crate::wallet::Wallet;
use anyhow::Result;
use colored::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

const CHECK_LAYER_FILE: &str = "check_layer.json";

pub struct VerifierState {
    pub wallet: Wallet,
    pub db: Arc<dyn Database>,
    pub check_layer: RwLock<Option<CheckLayer>>,
    pub known_peers: RwLock<Vec<SocketAddr>>,
}

impl VerifierState {
    pub fn new(wallet: Wallet, db: Arc<dyn Database>) -> Arc<Self> {
        let existing = CheckLayer::load(CHECK_LAYER_FILE).ok();
        Arc::new(VerifierState {
            wallet,
            db,
            check_layer: RwLock::new(existing),
            known_peers: RwLock::new(Vec::new()),
        })
    }
}

pub async fn run_server(port: u16, wallet: Wallet, db: Arc<dyn Database>) -> Result<()> {
    if wallet.govt_id_hash == "view" {
        return Err(anyhow::anyhow!(
            "UNAUTHORIZED: This wallet has 'view' access only.\nVerifier nodes require a verified Govt ID.\nCreate a new wallet with `cargo run -- wallet`."
        ));
    }

    let state = VerifierState::new(wallet.clone(), db);
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    println!("\n{}", "==================================================".bright_cyan());
    println!("{}", "   VERIFIER NODE  - ACTIVE LISTENER              ".bright_white().bold());
    println!("{}", "==================================================".bright_cyan());
    println!("  Node Address : {}", wallet.wallet_address[..16].to_string().yellow());
    println!("  Display Name : {}", wallet.display_name.cyan());
    println!("  Listening on : {}", addr.green().bold());
    println!("  DB Backend   : {}", state.db.name().magenta());
    println!("\n{}", "  Waiting for incoming transactions...".bright_white());
    println!("{}", "  -----------------------------------------".bright_black());

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        println!("\n  {} New connection from {}", "->".bright_cyan(), peer_addr);

        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer_addr, state).await {
                eprintln!("  {} Error handling {}: {}", "x".red(), peer_addr, e);
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    state: Arc<VerifierState>,
) -> Result<()> {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let message = read_message(&mut reader).await?;

    let response = process_message(message, peer_addr, Arc::clone(&state)).await?;
    write_message(&mut writer, &response).await?;

    Ok(())
}

async fn process_message(
    message: NetworkMessage,
    peer_addr: SocketAddr,
    state: Arc<VerifierState>,
) -> Result<NetworkMessage> {
    match message {
        NetworkMessage::PeerAnnounce { address, port, wallet_address, role } => {
            let peer_addr: SocketAddr = format!("{}:{}", address, port).parse()?;
            let mut peers = state.known_peers.write().await;
            if !peers.contains(&peer_addr) {
                peers.push(peer_addr);
                println!(
                    "  {} Registered peer: {} (role: {}, addr: {}...)",
                    "+".green(), peer_addr, role.yellow(), &wallet_address[..12]
                );
            }
            Ok(NetworkMessage::Ack {
                success: true,
                message: format!("Peer registered: {}", peer_addr),
            })
        }

        NetworkMessage::InitCheckLayer { check_layer, broadcaster_wallet } => {
            println!(
                "\n  {} InitCheckLayer received from {}...",
                "->".yellow(), &broadcaster_wallet[..16.min(broadcaster_wallet.len())]
            );
            println!("     Batch ID     : {}...", &check_layer.batch_id[..16]);
            println!("     Warehouses   : {}", check_layer.warehouses.len());
            println!("     Final Hash   : {}...", &check_layer.final_hash[..16]);
            println!("     Merkle Root  : {}...", &check_layer.merkle_root[..16]);

            let key = format!("check_layer:{}", check_layer.batch_id);
            let json = serde_json::to_string(&check_layer)?;
            state.db.store(&key, &json).await?;

            check_layer.save(CHECK_LAYER_FILE)?;
            *state.check_layer.write().await = Some(check_layer.clone());

            let mut all_hashes: Vec<String> = check_layer.warehouses.iter()
                .map(|w| w.w_a.clone()).collect();
            all_hashes.push(check_layer.distributor.w_a.clone());
            let computed_root = crate::crypto::compute_merkle_root(all_hashes);
            let approved = computed_root == check_layer.merkle_root;

            let payload_hash = hash_str(&serde_json::to_string(&check_layer)?);

            println!(
                "  {} Merkle verification: {}",
                "->".cyan(),
                if approved { "MATCH".green().to_string() } else { "MISMATCH".red().to_string() }
            );

            let vote = VoteResponse {
                approved,
                voter_address: state.wallet.wallet_address.clone(),
                payload_hash,
                timestamp: chrono::Utc::now().to_rfc3339(),
                reason: if !approved { Some("Merkle root mismatch".to_string()) } else { None },
            };

            println!("  {} Casting vote: {}", "vote".yellow(), if approved { "APPROVE".green().to_string() } else { "REJECT".red().to_string() });
            Ok(NetworkMessage::Vote(vote))
        }

        NetworkMessage::TransitionRequest {
            final_hash,
            agent_id_hash,
            wallet_address,
            layer_index,
            batch_id,
        } => {
            println!(
                "\n  {} TransitionRequest at index {} for batch {}...",
                "->".yellow(), layer_index, &batch_id[..16]
            );
            println!("     FINAL_HASH   : {}...", &final_hash[..16]);
            println!("     Agent ID     : {}...", &agent_id_hash[..16.min(agent_id_hash.len())]);
            println!("     Wallet Addr  : {}...", &wallet_address[..16.min(wallet_address.len())]);

            let key = format!("check_layer:{}", batch_id);
            let cl_json = state.db.fetch(&key).await?.unwrap_or_default();
            let cl_from_disk = CheckLayer::load(CHECK_LAYER_FILE).ok();

            let layer = if let Ok(cl) = serde_json::from_str::<CheckLayer>(&cl_json) {
                cl
            } else if let Some(cl) = cl_from_disk {
                cl
            } else {
                return Ok(NetworkMessage::Vote(VoteResponse {
                    approved: false,
                    voter_address: state.wallet.wallet_address.clone(),
                    payload_hash: hash_str(&final_hash),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    reason: Some("CheckLayer not found".to_string()),
                }));
            };

            let hash_ok = final_hash == layer.final_hash;
            let index_ok = layer_index == layer.current_index;
            let slot_ok = layer.warehouses.get(layer_index)
                .map(|w| w.status == crate::state::SlotStatus::Open)
                .unwrap_or(false)
                || (layer_index >= layer.warehouses.len()
                    && layer.distributor.status == crate::state::SlotStatus::Open);

            let approved = hash_ok && index_ok && slot_ok;
            let payload_hash = hash_str(&format!("{}{}{}", final_hash, agent_id_hash, layer_index));

            println!("  {} Hash OK: {} | Index OK: {} | Slot OK: {}",
                "->".cyan(),
                if hash_ok { "OK" } else { "FAIL" },
                if index_ok { "OK" } else { "FAIL" },
                if slot_ok { "OK" } else { "FAIL" }
            );
            println!("  {} Vote: {}", "vote".yellow(),
                if approved { "APPROVE".green().to_string() } else { "REJECT".red().to_string() }
            );

            Ok(NetworkMessage::Vote(VoteResponse {
                approved,
                voter_address: state.wallet.wallet_address.clone(),
                payload_hash,
                timestamp: chrono::Utc::now().to_rfc3339(),
                reason: if !approved {
                    Some(format!("hash_ok={} index_ok={} slot_ok={}", hash_ok, index_ok, slot_ok))
                } else {
                    None
                },
            }))
        }

        NetworkMessage::DistributorFinalize {
            final_hash,
            agent_id_hash,
            wallet_address,
            small_box_tag,
            batch_id,
        } => {
            println!(
                "\n  {} DistributorFinalize for batch {}...",
                "->".yellow(), &batch_id[..16]
            );
            println!("     Tag ID       : {}...", &small_box_tag.tag_id[..16]);
            println!("     Box Count    : {}", small_box_tag.box_count);
            println!("     Receiver     : {}...", &small_box_tag.receiver_wallet_hash[..16]);

            let key = format!("check_layer:{}", batch_id);
            let cl_json = state.db.fetch(&key).await?.unwrap_or_default();
            let cl_from_disk = CheckLayer::load(CHECK_LAYER_FILE).ok();

            let layer = if let Ok(cl) = serde_json::from_str::<CheckLayer>(&cl_json) {
                cl
            } else if let Some(cl) = cl_from_disk {
                cl
            } else {
                return Ok(NetworkMessage::Vote(VoteResponse {
                    approved: false,
                    voter_address: state.wallet.wallet_address.clone(),
                    payload_hash: hash_str(&final_hash),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    reason: Some("CheckLayer not found".to_string()),
                }));
            };

            let hash_ok = final_hash == layer.final_hash;
            let slot_ok = layer.distributor.status == crate::state::SlotStatus::Open;
            let computed_root = crate::crypto::compute_merkle_root(small_box_tag.box_hashes.clone());
            let merkle_ok = computed_root == small_box_tag.merkle_root;

            let approved = hash_ok && slot_ok && merkle_ok;
            let payload_hash = hash_str(&serde_json::to_string(&small_box_tag)?);

            println!("  {} Hash OK: {} | Slot OK: {} | Merkle OK: {}",
                "->".cyan(),
                if hash_ok { "OK" } else { "FAIL" },
                if slot_ok { "OK" } else { "FAIL" },
                if merkle_ok { "OK" } else { "FAIL" }
            );
            println!("  {} Vote: {}", "vote".yellow(),
                if approved { "APPROVE".green().to_string() } else { "REJECT".red().to_string() }
            );

            Ok(NetworkMessage::Vote(VoteResponse {
                approved,
                voter_address: state.wallet.wallet_address.clone(),
                payload_hash,
                timestamp: chrono::Utc::now().to_rfc3339(),
                reason: if !approved {
                    Some(format!("hash_ok={} slot_ok={} merkle_ok={}", hash_ok, slot_ok, merkle_ok))
                } else {
                    None
                },
            }))
        }

        NetworkMessage::ReceiverSubmit {
            wallet_address_hash,
            inside_hashes,
            final_hash,
            signature,
            batch_id,
        } => {
            println!(
                "\n  {} ReceiverSubmit for batch {}...",
                "->".yellow(), &batch_id[..16]
            );
            println!("     Receiver     : {}...", &wallet_address_hash[..16]);
            println!("     Box Count    : {}", inside_hashes.len());

            let key = format!("check_layer:{}", batch_id);
            let cl_json = state.db.fetch(&key).await?.unwrap_or_default();
            let cl_from_disk = CheckLayer::load(CHECK_LAYER_FILE).ok();

            let layer = if let Ok(cl) = serde_json::from_str::<CheckLayer>(&cl_json) {
                cl
            } else if let Some(cl) = cl_from_disk {
                cl
            } else {
                return Ok(NetworkMessage::Vote(VoteResponse {
                    approved: false,
                    voter_address: state.wallet.wallet_address.clone(),
                    payload_hash: hash_str(&final_hash),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    reason: Some("CheckLayer not found".to_string()),
                }));
            };

            let tag = layer.small_box_tags.iter()
                .find(|t| t.receiver_wallet_hash == wallet_address_hash);

            let (approved, reason) = if let Some(tag) = tag {
                let computed_root = crate::crypto::compute_merkle_root(inside_hashes.clone());
                let merkle_ok = computed_root == tag.merkle_root;
                let count_ok = inside_hashes.len() as u32 == tag.box_count;

                let _data_hash = hash_str(&format!("{}{}",
                    final_hash,
                    inside_hashes.join("")
                ));
                let sig_format_ok = signature.len() == 128;

                println!("  {} Merkle OK: {} | Count OK: {} | Sig Format OK: {}",
                    "->".cyan(),
                    if merkle_ok { "OK" } else { "FAIL" },
                    if count_ok { "OK" } else { "FAIL" },
                    if sig_format_ok { "OK" } else { "FAIL" }
                );

                if merkle_ok && count_ok {
                    (true, None)
                } else {
                    (false, Some(format!(
                        "merkle_ok={} count_ok={}. Expected root: {}..., got: {}...",
                        merkle_ok, count_ok,
                        &tag.merkle_root[..16],
                        &computed_root[..16]
                    )))
                }
            } else {
                (false, Some("No small box tag found for this receiver".to_string()))
            };

            let payload_hash = hash_str(&format!("{}{}", wallet_address_hash, final_hash));
            println!("  {} Vote: {}", "vote".yellow(),
                if approved { "APPROVE".green().to_string() } else { "REJECT".red().to_string() }
            );

            Ok(NetworkMessage::Vote(VoteResponse {
                approved,
                voter_address: state.wallet.wallet_address.clone(),
                payload_hash,
                timestamp: chrono::Utc::now().to_rfc3339(),
                reason,
            }))
        }

        NetworkMessage::StateRequest { batch_id } => {
            let key = format!("check_layer:{}", batch_id);
            let cl_json = state.db.fetch(&key).await?.unwrap_or_default();
            let layer_opt = serde_json::from_str::<CheckLayer>(&cl_json)
                .ok()
                .or_else(|| CheckLayer::load(CHECK_LAYER_FILE).ok());

            match layer_opt {
                Some(layer) => Ok(NetworkMessage::StateResponse { check_layer: layer }),
                None => Ok(NetworkMessage::Ack {
                    success: false,
                    message: "CheckLayer not found".to_string(),
                }),
            }
        }

        other => {
            eprintln!("  {} Unhandled message type: {:?}", "!".yellow(), std::mem::discriminant(&other));
            Ok(NetworkMessage::Ack {
                success: false,
                message: "Unhandled message type".to_string(),
            })
        }
    }
}
