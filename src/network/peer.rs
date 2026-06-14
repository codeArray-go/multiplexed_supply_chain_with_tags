use crate::network::message::NetworkMessage;
use anyhow::Result;
use colored::*;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT_MS: u64 = 2000;
const READ_TIMEOUT_MS: u64 = 5000;

pub async fn send_and_receive(
    addr: SocketAddr,
    message: &NetworkMessage,
) -> Result<NetworkMessage> {
    let stream = timeout(
        Duration::from_millis(CONNECT_TIMEOUT_MS),
        TcpStream::connect(addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Connection timeout to {}", addr))?
    .map_err(|e| anyhow::anyhow!("Cannot connect to {}: {}", addr, e))?;

    let (mut reader, mut writer) = tokio::io::split(stream);

    let frame = message.to_framed_bytes()?;
    writer.write_all(&frame).await?;

    let response = timeout(Duration::from_millis(READ_TIMEOUT_MS), async {
        read_message(&mut reader).await
    })
    .await
    .map_err(|_| anyhow::anyhow!("Read timeout from {}", addr))??;

    Ok(response)
}

pub async fn broadcast(
    peers: &[SocketAddr],
    message: &NetworkMessage,
) -> Vec<(SocketAddr, Result<NetworkMessage>)> {
    let mut handles = Vec::new();

    for &peer in peers {
        let msg = message.clone();
        let handle = tokio::spawn(async move {
            let result = send_and_receive(peer, &msg).await;
            (peer, result)
        });
        handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("{} Task panic: {}", "!".yellow(), e);
            }
        }
    }

    results
}

pub async fn broadcast_and_collect_votes(
    peers: &[SocketAddr],
    message: &NetworkMessage,
) -> Vec<crate::consensus::vote::VoteResponse> {
    let results = broadcast(peers, message).await;
    let mut votes = Vec::new();

    for (addr, result) in results {
        match result {
            Ok(NetworkMessage::Vote(v)) => {
                println!(
                    "  {} Vote from {} -> {}",
                    "->".cyan(),
                    addr,
                    if v.approved { "APPROVE".green().to_string() } else { "REJECT".red().to_string() }
                );
                votes.push(v);
            }
            Ok(other) => {
                eprintln!("  {} Unexpected response from {}: {:?}", "!".yellow(), addr, other);
            }
            Err(e) => {
                eprintln!("  {} {} unreachable: {}", "x".red(), addr, e);
            }
        }
    }

    votes
}

pub async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<NetworkMessage> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 10 * 1024 * 1024 {
        return Err(anyhow::anyhow!("Message too large: {} bytes", len));
    }

    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;

    let msg: NetworkMessage = serde_json::from_slice(&body)?;
    Ok(msg)
}

pub async fn write_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    message: &NetworkMessage,
) -> Result<()> {
    let frame = message.to_framed_bytes()?;
    writer.write_all(&frame).await?;
    Ok(())
}


pub fn parse_peer_addrs(peers_str: &str) -> Vec<SocketAddr> {
    peers_str
        .split(',')
        .filter_map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed.parse::<SocketAddr>().ok().or_else(|| {
                eprintln!("Could not parse peer address: '{}'", trimmed);
                None
            })
        })
        .collect()
}
