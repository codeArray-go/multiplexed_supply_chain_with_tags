use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResponse {
    pub approved: bool,
    pub voter_address: String,
    pub payload_hash: String,
    pub timestamp: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VoteResult {
    pub passed: bool,
    pub approve_count: usize,
    pub reject_count: usize,
    pub total_voters: usize,
    pub threshold_pct: f64,
    pub actual_pct: f64,
}

impl std::fmt::Display for VoteResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}/{}] {:.1}% (threshold: {:.1}%) -> {}",
            self.approve_count,
            self.total_voters,
            self.actual_pct * 100.0,
            self.threshold_pct * 100.0,
            if self.passed { "PASSED" } else { "REJECTED" }
        )
    }
}

pub fn tally_votes(votes: &[VoteResponse], total_nodes: usize) -> VoteResult {
    let approve_count = votes.iter().filter(|v| v.approved).count();
    let reject_count = votes.len() - approve_count;
    let threshold = 0.67_f64;

    let actual_pct = if total_nodes == 0 {
        0.0
    } else {
        approve_count as f64 / total_nodes as f64
    };

    VoteResult {
        passed: actual_pct > threshold,
        approve_count,
        reject_count,
        total_voters: total_nodes,
        threshold_pct: threshold,
        actual_pct,
    }
}

#[derive(Debug)]
struct VoteSession {
    votes: Vec<VoteResponse>,
    total_expected: usize,
}

pub struct VoteCollector {
    sessions: RwLock<HashMap<String, VoteSession>>,
}

impl VoteCollector {
    pub fn new() -> Self {
        VoteCollector {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn start_session(&self, payload_hash: &str, total_expected: usize) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            payload_hash.to_string(),
            VoteSession {
                votes: Vec::new(),
                total_expected,
            },
        );
    }

    pub async fn record_vote(&self, vote: VoteResponse) -> Result<Option<VoteResult>> {
        let mut sessions = self.sessions.write().await;
        let session = match sessions.get_mut(&vote.payload_hash) {
            Some(s) => s,
            None => {
                sessions.insert(
                    vote.payload_hash.clone(),
                    VoteSession {
                        votes: Vec::new(),
                        total_expected: 1,
                    },
                );
                sessions.get_mut(&vote.payload_hash).unwrap()
            }
        };

        let already_voted = session.votes.iter().any(|v| v.voter_address == vote.voter_address);
        if !already_voted {
            session.votes.push(vote);
        }

        if session.votes.len() >= session.total_expected {
            let result = tally_votes(&session.votes, session.total_expected);
            return Ok(Some(result));
        }

        Ok(None)
    }

    pub async fn force_tally(&self, payload_hash: &str) -> Option<VoteResult> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(payload_hash)?;
        Some(tally_votes(&session.votes, session.total_expected))
    }

    pub async fn close_session(&self, payload_hash: &str) {
        self.sessions.write().await.remove(payload_hash);
    }
}

impl Default for VoteCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vote(approved: bool, voter: &str) -> VoteResponse {
        VoteResponse {
            approved,
            voter_address: voter.to_string(),
            payload_hash: "testhash".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reason: None,
        }
    }

    #[test]
    fn test_67_percent_threshold() {
        let votes = vec![
            make_vote(true, "A"),
            make_vote(true, "B"),
            make_vote(false, "C"),
        ];
        let result = tally_votes(&votes, 3);
        assert!(!result.passed);
    }

    #[test]
    fn test_passes_with_4_of_5() {
        let votes = vec![
            make_vote(true, "A"),
            make_vote(true, "B"),
            make_vote(true, "C"),
            make_vote(true, "D"),
            make_vote(false, "E"),
        ];
        let result = tally_votes(&votes, 5);
        assert!(result.passed);
    }

    #[test]
    fn test_unanimous() {
        let votes = vec![
            make_vote(true, "A"),
            make_vote(true, "B"),
            make_vote(true, "C"),
        ];
        let result = tally_votes(&votes, 3);
        assert!(result.passed);
    }
}
