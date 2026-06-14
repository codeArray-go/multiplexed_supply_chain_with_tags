mod consensus;
mod crypto;
mod db;
mod network;
mod prompt;
mod state;
mod wallet;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use std::net::SocketAddr;
use std::sync::Arc;

use consensus::{tally_votes, VoteResult};
use crypto::hash::hash_str;
use crypto::compute_merkle_root;
use db::connect_or_mock;
use network::{
    message::NetworkMessage,
    peer::{broadcast_and_collect_votes, parse_peer_addrs},
    run_server,
};
use state::{CheckLayer, SmallBoxTag};
use wallet::{create_wallet, load_wallet};

#[derive(Parser, Debug)]
#[command(
    name = "supply-chain-blockchain",
    about = "Supply Chain Blockchain — Multi-terminal tracking system",
    version = "0.1.0"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, default_value = "wallet.json")]
    wallet: String,

    #[arg(long, default_value = "9000")]
    port: u16,

    #[arg(long, default_value = "127.0.0.1:9000")]
    peers: String,

    #[arg(long, default_value = "check_layer.json")]
    layer_file: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Wallet,
    Verifier,
    Manufacturer,
    Warehouse,
    Distributor,
    Receiver,
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    print_banner();

    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    let verify_url = std::env::var("DB_VERIFY_URL").ok();
    let storage_url = std::env::var("DB_STORAGE_URL").ok();
    let db = connect_or_mock(verify_url.as_deref(), storage_url.as_deref()).await;
    let db: Arc<dyn db::Database> = Arc::from(db);

    let peers = parse_peer_addrs(&cli.peers);

    match cli.command {
        Commands::Wallet      => cmd_wallet(db, &cli.wallet).await?,
        Commands::Verifier    => cmd_verifier(cli.port, &cli.wallet, db).await?,
        Commands::Manufacturer => cmd_manufacturer(&cli.wallet, &peers, &cli.layer_file, db).await?,
        Commands::Warehouse   => cmd_warehouse(&cli.wallet, &peers, &cli.layer_file).await?,
        Commands::Distributor => cmd_distributor(&cli.wallet, &peers, &cli.layer_file).await?,
        Commands::Receiver    => cmd_receiver(&cli.wallet, &peers, &cli.layer_file).await?,
        Commands::Status      => cmd_status(&cli.layer_file)?,
    }

    Ok(())
}


async fn cmd_wallet(db: Arc<dyn db::Database>, wallet_path: &str) -> Result<()> {
    create_wallet(db, wallet_path).await?;
    Ok(())
}

async fn cmd_verifier(port: u16, wallet_path: &str, db: Arc<dyn db::Database>) -> Result<()> {
    let w = load_wallet(wallet_path)?;
    run_server(port, w, db).await?;
    Ok(())
}

async fn cmd_manufacturer(
    wallet_path: &str,
    peers: &[SocketAddr],
    layer_file: &str,
    db: Arc<dyn db::Database>,
) -> Result<()> {
    let w = load_wallet(wallet_path)?;
    section_header("MANUFACTURER — Initialize Supply Chain");

    println!("  Wallet       : {}", w.display_name.cyan());
    println!("  Address      : {}...\n", &w.wallet_address[..16]);

    let final_hash_input = prompt::input_optional("Batch FINAL_HASH (Enter to auto-generate)")?;
    let final_hash = if final_hash_input.is_empty() {
        let batch_name = prompt::input("Batch name / description (will be hashed)")?;
        let h = hash_str(batch_name.trim());
        println!("  Auto-generated FINAL_HASH: {}...", &h[..32]);
        h
    } else {
        final_hash_input
    };

    let agent_id = prompt::input("Initial agent ID (first warehouse worker)")?;

    println!("\n{}", "  Enter warehouse addresses one by one.".bright_white());
    println!("{}", "  Press Enter with no input to finish.\n".bright_black());

    let mut warehouse_addresses: Vec<String> = Vec::new();
    let mut wh_num = 0;
    loop {
        let addr = prompt::input_optional(&format!("Warehouse {} address", wh_num))?;
        if addr.is_empty() {
            if warehouse_addresses.is_empty() {
                println!("  {} At least one warehouse is required.", "!".yellow());
                continue;
            }
            break;
        }
        warehouse_addresses.push(addr);
        wh_num += 1;
    }

    let distributor_address = prompt::input("Distributor address")?;

    let layer = CheckLayer::new(
        w.wallet_address.clone(),
        warehouse_addresses,
        distributor_address,
        final_hash.clone(),
        agent_id,
    )?;

    layer.save(layer_file)?;

    println!("\n  CheckLayer created:");
    let display = layer.display_map();
    println!("{}", serde_json::to_string_pretty(&display)?.bright_white());

    let key = format!("check_layer:{}", layer.batch_id);
    db.store(&key, &serde_json::to_string(&layer)?).await?;

    if peers.is_empty() {
        println!("\n  {} No peers — skipping broadcast.", "i".cyan());
    } else {
        let msg = NetworkMessage::InitCheckLayer {
            check_layer: layer.clone(),
            broadcaster_wallet: w.wallet_address.clone(),
        };
        println!("\n  Broadcasting to {} verifier(s)...", peers.len());
        let votes = broadcast_and_collect_votes(peers, &msg).await;
        let result = tally_votes(&votes, peers.len());
        print_vote_result(&result);
    }

    println!("\n  Batch ID : {}", layer.batch_id.bright_white());
    println!("  Saved    : {}\n", layer_file.green());

    Ok(())
}

async fn cmd_warehouse(
    wallet_path: &str,
    peers: &[SocketAddr],
    layer_file: &str,
) -> Result<()> {
    let w = load_wallet(wallet_path)?;
    section_header("WAREHOUSE WORKER — Submit Handoff");

    println!("  Wallet       : {}", w.display_name.cyan());
    println!("  Address      : {}...\n", &w.wallet_address[..16]);

    let layer = CheckLayer::load(layer_file)
        .map_err(|e| anyhow::anyhow!("{}\n  Run `cargo run -- manufacturer` first.", e))?;

    println!("  Batch ID     : {}...", &layer.batch_id[..16].yellow());
    println!("  Current Index: {}", layer.current_index.to_string().cyan());
    println!("  Final Hash   : {}...\n", &layer.final_hash[..16]);

    let final_hash = prompt::input("Batch FINAL_HASH")?;
    let agent_id_raw = prompt::input("Your Agent ID")?;
    let agent_id_hash = hash_str(agent_id_raw.trim());

    let msg = NetworkMessage::TransitionRequest {
        final_hash: final_hash.trim().to_string(),
        agent_id_hash: agent_id_hash.clone(),
        wallet_address: w.wallet_address.clone(),
        layer_index: layer.current_index,
        batch_id: layer.batch_id.clone(),
    };

    if peers.is_empty() {
        println!("\n  {} No peers — applying transition locally.", "i".cyan());
        let mut layer = layer;
        layer.apply_warehouse_transition(
            final_hash.trim(), &agent_id_hash, &w.wallet_address, 1, 1,
        )?;
        layer.save(layer_file)?;
        println!("  Transition applied (local mode).");
        print_layer_state(&layer);
        return Ok(());
    }

    println!("\n  Broadcasting to {} verifier(s)...", peers.len());
    let votes = broadcast_and_collect_votes(peers, &msg).await;
    let result = tally_votes(&votes, peers.len());
    print_vote_result(&result);

    if result.passed {
        let mut layer = layer;
        layer.apply_warehouse_transition(
            final_hash.trim(),
            &agent_id_hash,
            &w.wallet_address,
            result.approve_count,
            result.total_voters,
        )?;
        layer.save(layer_file)?;
        println!("\n  State transition applied!");
        print_layer_state(&layer);
    } else {
        println!("\n  Consensus FAILED — no state change.");
    }

    Ok(())
}

async fn cmd_distributor(
    wallet_path: &str,
    peers: &[SocketAddr],
    layer_file: &str,
) -> Result<()> {
    let w = load_wallet(wallet_path)?;
    section_header("DISTRIBUTOR — Finalization & Small Box Tagging");

    println!("  Wallet       : {}", w.display_name.cyan());
    println!("  Address      : {}...\n", &w.wallet_address[..16]);

    let layer = CheckLayer::load(layer_file)
        .map_err(|e| anyhow::anyhow!("{}\n  Run manufacturer + warehouses first.", e))?;

    println!("  Batch ID     : {}...", &layer.batch_id[..16].yellow());
    println!("  Final Hash   : {}...", &layer.final_hash[..16]);
    println!("  Distributor  : {}\n", layer.distributor.status.to_string().cyan());

    let final_hash = prompt::input("Batch FINAL_HASH")?;
    let agent_id_raw = prompt::input("Your Agent ID")?;
    let agent_id_hash = hash_str(agent_id_raw.trim());

    println!("\n{}", "  -- Small Box Chain --".bright_cyan());
    let receiver_wallet_raw = prompt::input("Receiver wallet address")?;
    let receiver_wallet_hash = hash_str(receiver_wallet_raw.trim());

    let box_count = prompt::input_u32("Number of small boxes")?;

    println!("\n  Enter each box hash (Enter to auto-generate):");
    let mut box_hashes: Vec<String> = Vec::new();
    for i in 0..box_count {
        let bh = prompt::input_optional(&format!("Box {} hash", i + 1))?;
        if bh.is_empty() {
            let auto = hash_str(&format!(
                "box_{}_{}_{}", i, layer.batch_id,
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(i as i64)
            ));
            println!("    Auto: {}...", &auto[..32]);
            box_hashes.push(auto);
        } else {
            box_hashes.push(bh);
        }
    }

    let small_box_tag = SmallBoxTag::new(receiver_wallet_hash, box_hashes);

    println!("\n  Small Box Tag:");
    println!("     Tag ID   : {}...", &small_box_tag.tag_id[..16]);
    println!("     Boxes    : {}", small_box_tag.box_count);
    println!("     Merkle   : {}...", &small_box_tag.merkle_root[..16]);

    let msg = NetworkMessage::DistributorFinalize {
        final_hash: final_hash.trim().to_string(),
        agent_id_hash: agent_id_hash.clone(),
        wallet_address: w.wallet_address.clone(),
        small_box_tag: small_box_tag.clone(),
        batch_id: layer.batch_id.clone(),
    };

    if peers.is_empty() {
        println!("\n  {} No peers — applying locally.", "i".cyan());
        let mut layer = layer;
        layer.apply_distributor_finalization(
            final_hash.trim(), &agent_id_hash, &w.wallet_address,
            small_box_tag, 1, 1,
        )?;
        layer.save(layer_file)?;
        println!("  Distributor finalization applied (local).");
        return Ok(());
    }

    println!("\n  Broadcasting to {} verifier(s)...", peers.len());
    let votes = broadcast_and_collect_votes(peers, &msg).await;
    let result = tally_votes(&votes, peers.len());
    print_vote_result(&result);

    if result.passed {
        let mut layer = layer;
        layer.apply_distributor_finalization(
            final_hash.trim(),
            &agent_id_hash,
            &w.wallet_address,
            small_box_tag,
            result.approve_count,
            result.total_voters,
        )?;
        layer.save(layer_file)?;
        println!("\n  Distributor finalization complete!");
        print_layer_state(&layer);
    } else {
        println!("\n  Consensus FAILED.");
    }

    Ok(())
}

async fn cmd_receiver(
    wallet_path: &str,
    peers: &[SocketAddr],
    layer_file: &str,
) -> Result<()> {
    let w = load_wallet(wallet_path)?;
    section_header("RECEIVER — Final Verification");

    println!("  Wallet       : {}", w.display_name.cyan());
    println!("  Address      : {}...\n", &w.wallet_address[..16]);

    let layer = CheckLayer::load(layer_file)
        .map_err(|e| anyhow::anyhow!("{}\n  Run distributor first.", e))?;

    println!("  Batch ID     : {}...", &layer.batch_id[..16].yellow());
    println!("  Small Box Tags: {}\n", layer.small_box_tags.len().to_string().cyan());

    let final_hash = prompt::input("Small box batch FINAL_HASH")?;
    let box_count = prompt::input_usize("How many boxes did you receive?")?;

    println!("\n  Enter each received box hash:");
    let mut inside_hashes: Vec<String> = Vec::new();
    for i in 0..box_count {
        let bh = prompt::input(&format!("Box {} hash", i + 1))?;
        inside_hashes.push(bh);
    }

    let message_to_sign = format!("{}{}", final_hash.trim(), inside_hashes.join(""));
    let signature = w.sign(message_to_sign.as_bytes())?;
    println!("\n  Signed payload: {}...", &signature[..32]);

    let wallet_address_hash = hash_str(&w.wallet_address);

    let msg = NetworkMessage::ReceiverSubmit {
        wallet_address_hash: wallet_address_hash.clone(),
        inside_hashes: inside_hashes.clone(),
        final_hash: final_hash.trim().to_string(),
        signature: signature.clone(),
        batch_id: layer.batch_id.clone(),
    };

    if peers.is_empty() {
        println!("\n  {} No peers — running local verification.", "i".cyan());
        let computed_root = compute_merkle_root(inside_hashes.clone());
        let tag = layer.small_box_tags.iter()
            .find(|t| t.receiver_wallet_hash == wallet_address_hash);

        let matched = tag.map(|t| {
            t.merkle_root == computed_root && t.box_count == inside_hashes.len() as u32
        }).unwrap_or(false);

        if matched {
            let mut layer = layer;
            layer.apply_receiver_finalization(
                &wallet_address_hash, &inside_hashes, final_hash.trim(), 1, 1,
            )?;
            layer.save(layer_file)?;
            println!(
                "\n  ====================================\n\
                 \n  CHAIN APPENDED — Shipment Complete!\n\
                 \n  ====================================\n"
            );
        } else {
            println!("\n  FLAGGED — Hash mismatch or no matching tag.");
        }
        return Ok(());
    }

    println!("\n  Broadcasting to {} verifier(s)...", peers.len());
    let votes = broadcast_and_collect_votes(peers, &msg).await;
    let result = tally_votes(&votes, peers.len());
    print_vote_result(&result);

    if result.passed {
        let mut layer = layer;
        match layer.apply_receiver_finalization(
            &wallet_address_hash, &inside_hashes, final_hash.trim(),
            result.approve_count, result.total_voters,
        ) {
            Ok(true) => {
                layer.save(layer_file)?;
                println!(
                    "\n  ====================================\n\
                     \n  CHAIN APPENDED — Shipment Complete!\n\
                     \n  ====================================\n"
                );
            }
            Ok(false) => {
                println!("\n  FLAGGED — Data mismatch detected!");
            }
            Err(e) => {
                println!("\n  Error: {}", e);
            }
        }
    } else {
        println!("\n  Consensus FAILED — shipment flagged!");
    }

    Ok(())
}

fn cmd_status(layer_file: &str) -> Result<()> {
    section_header("CHECK LAYER STATUS");
    let layer = CheckLayer::load(layer_file)?;
    let display = layer.display_map();
    println!("{}", serde_json::to_string_pretty(&display)?.bright_white());
    println!("\n  Batch ID     : {}", layer.batch_id.yellow());
    println!("  Transitions  : {}", layer.transition_log.len().to_string().cyan());
    println!("  Small Tags   : {}", layer.small_box_tags.len().to_string().cyan());
    println!("  Finalized    : {}", layer.finalized.to_string().green());
    Ok(())
}


fn print_banner() {
    println!();
    println!("{}", "  +==================================================+".bright_cyan());
    println!("{}", "  |   Supply Chain Blockchain - Rust v0.1.0          |".bright_white().bold());
    println!("{}", "  |      Modular · Multi-terminal · Consensus         |".bright_black());
    println!("{}", "  +==================================================+".bright_cyan());
    println!();
}

fn section_header(title: &str) {
    let border = "-".repeat(title.len() + 4);
    println!("\n  +{}+", border.bright_cyan());
    println!("  |  {}  |", title.bright_white().bold());
    println!("  +{}+\n", border.bright_cyan());
}

fn print_vote_result(result: &VoteResult) {
    let status = if result.passed {
        "CONSENSUS REACHED".green().bold().to_string()
    } else {
        "CONSENSUS FAILED".red().bold().to_string()
    };
    println!("\n  {}", status);
    println!(
        "  Votes: {}/{} ({:.1}%) — Threshold: {:.0}%",
        result.approve_count,
        result.total_voters,
        result.actual_pct * 100.0,
        result.threshold_pct * 100.0
    );
}

fn print_layer_state(layer: &CheckLayer) {
    println!("\n  Current State:");
    let mut keys: Vec<_> = layer.display_map().into_iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in keys {
        println!("    {}: {}", k.cyan(), v);
    }
}
