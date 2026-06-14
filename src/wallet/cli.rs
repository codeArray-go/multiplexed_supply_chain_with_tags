use crate::crypto::hash::hash_str;
use crate::db::Database;
use crate::prompt;
use anyhow::{anyhow, Result};
use colored::*;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WalletRole {
    Citizen,
    Manufacturer,
    Distributor,
    Verifier,
    Warehouse,
    Receiver,
}

impl std::fmt::Display for WalletRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletRole::Citizen => write!(f, "citizen"),
            WalletRole::Manufacturer => write!(f, "manufacturer"),
            WalletRole::Distributor => write!(f, "distributor"),
            WalletRole::Verifier => write!(f, "verifier"),
            WalletRole::Warehouse => write!(f, "warehouse"),
            WalletRole::Receiver => write!(f, "receiver"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub govt_id_hash: String,
    pub private_key_hex: String,
    pub public_key_hex: String,
    pub wallet_address: String,
    pub role: WalletRole,
    pub display_name: String,
    pub created_at: String,
}

impl Wallet {
    pub fn address_from_pubkey(pubkey_hex: &str) -> String {
        hash_str(pubkey_hex)
    }

    pub fn is_verified(&self) -> bool {
        self.govt_id_hash != "view"
    }

    pub fn sign(&self, message: &[u8]) -> Result<String> {
        use ed25519_dalek::Signer;
        let key_bytes = hex::decode(&self.private_key_hex)?;
        let key_array: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| anyhow!("Invalid private key length"))?;
        let signing_key = SigningKey::from_bytes(&key_array);
        let signature = signing_key.sign(message);
        Ok(hex::encode(signature.to_bytes()))
    }

    pub fn verify_signature(&self, message: &[u8], signature_hex: &str) -> Result<bool> {
        use ed25519_dalek::Verifier;
        use ed25519_dalek::Signature;
        let pub_bytes = hex::decode(&self.public_key_hex)?;
        let pub_array: [u8; 32] = pub_bytes
            .try_into()
            .map_err(|_| anyhow!("Invalid public key length"))?;
        let verifying_key = VerifyingKey::from_bytes(&pub_array)
            .map_err(|e| anyhow!("Invalid public key: {}", e))?;
        let sig_bytes = hex::decode(signature_hex)?;
        let sig_array: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| anyhow!("Invalid signature length"))?;
        let signature = Signature::from_bytes(&sig_array);
        Ok(verifying_key.verify(message, &signature).is_ok())
    }
}

pub async fn create_wallet(db: Arc<dyn Database>, wallet_path: &str) -> Result<Wallet> {
    println!("\n{}", "=======================================".bright_cyan());
    println!("{}", " Supply Chain Wallet Creation CLI  ".bright_white().bold());
    println!("{}\n", "=======================================".bright_cyan());

    println!("  Aadhaar Number is mandatory for wallet creation.");
    let aadhaar = loop {
        let a = prompt::input("Aadhaar Number")?;
        if a.len() >= 4 {
            break a;
        }
        println!("  {} Must be at least 4 characters.", "!".yellow());
    };

    let govt_id_input = prompt::input_optional("Govt ID")?;

    let (govt_id_hash, role, display_name) = if govt_id_input.is_empty() {
        println!("\n{}", "  No Govt ID provided. Select your role:".yellow());

        let choices = &[
            "Verifier Node",
            "Warehouse Worker",
            "Receiver",
            "Manufacturer (company verification required)",
            "Distributor (company verification required)",
            "View-only citizen",
        ];

        let choice = prompt::select("Select role", choices)?;

        match choice {
            0 => {
                let h = hash_str(aadhaar.trim());
                let name = prompt::input_or_default("Display name", "Verifier Node")?;
                (h, WalletRole::Verifier, name)
            }
            1 => {
                let h = hash_str(aadhaar.trim());
                let name = prompt::input_or_default("Display name", "Warehouse Worker")?;
                (h, WalletRole::Warehouse, name)
            }
            2 => {
                let h = hash_str(aadhaar.trim());
                let name = prompt::input("Your receiver name")?;
                (h, WalletRole::Receiver, name)
            }
            3 => {
                let company = prompt::input("Company Name")?;
                let mfr_govt_id = prompt::input("Manufacturer Govt ID (from DB)")?;

                let gid_hash = hash_str(mfr_govt_id.trim());
                let company_hash = hash_str(company.trim());

                print!("\n  Verifying with DB... ");
                let ok = db.verify_manufacturer(&gid_hash, &company_hash).await?;
                if !ok {
                    return Err(anyhow!(
                        "Manufacturer verification FAILED for '{}' / '{}'.\nFor testing use: Govt ID = MFR001, Company = AcmeCorp",
                        mfr_govt_id, company
                    ));
                }
                println!("{}", "Verified!".green().bold());
                (gid_hash, WalletRole::Manufacturer, company)
            }
            4 => {
                let dist_govt_id = prompt::input("Distributor Govt ID (from DB)")?;

                let gid_hash = hash_str(dist_govt_id.trim());

                print!("\n  Verifying distributor... ");
                let ok = db.verify_distributor(&gid_hash).await?;
                if !ok {
                    return Err(anyhow!(
                        "Distributor verification FAILED.\nFor testing use: Govt ID = DIST001"
                    ));
                }
                println!("{}", "Verified!".green().bold());
                let name = prompt::input("Company / display name")?;
                (gid_hash, WalletRole::Distributor, name)
            }
            _ => {
                ("view".to_string(), WalletRole::Citizen, "Citizen (view-only)".to_string())
            }
        }
    } else {
        let h = hash_str(govt_id_input.trim());
        let name = prompt::input("Your display name")?;
        (h, WalletRole::Verifier, name)
    };

    let mut rng = OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    let private_key_hex = hex::encode(signing_key.to_bytes());
    let public_key_hex = hex::encode(verifying_key.to_bytes());
    let wallet_address = Wallet::address_from_pubkey(&public_key_hex);

    let wallet = Wallet {
        govt_id_hash: govt_id_hash.clone(),
        private_key_hex,
        public_key_hex,
        wallet_address: wallet_address.clone(),
        role: role.clone(),
        display_name: display_name.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string_pretty(&wallet)?;
    std::fs::write(wallet_path, &json)?;

    println!("\n{}", "=======================================".green());
    println!("{}", " Wallet Created Successfully!".green().bold());
    println!("{}", "=======================================".green());
    println!("  Role         : {}", role.to_string().yellow().bold());
    println!("  Display Name : {}", display_name.cyan());
    println!(
        "  Govt ID Hash : {}...",
        &govt_id_hash[..16.min(govt_id_hash.len())]
    );
    println!("  Address      : {}...", &wallet_address[..16]);
    println!("  Saved to     : {}\n", wallet_path.bright_white());

    Ok(wallet)
}

pub fn load_wallet(path: &str) -> Result<Wallet> {
    if !Path::new(path).exists() {
        return Err(anyhow!(
            "Wallet file '{}' not found.\nRun `cargo run -- wallet` first to create one.",
            path
        ));
    }
    let contents = std::fs::read_to_string(path)?;
    let wallet: Wallet = serde_json::from_str(&contents)?;
    Ok(wallet)
}

pub fn load_wallet_with_role(path: &str, required_role: &WalletRole) -> Result<Wallet> {
    let wallet = load_wallet(path)?;
    if &wallet.role != required_role {
        return Err(anyhow!(
            "This wallet has role '{}' but '{}' is required.\nCreate a new wallet with `cargo run -- wallet`.",
            wallet.role, required_role
        ));
    }
    Ok(wallet)
}
