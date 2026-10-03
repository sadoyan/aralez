use instant_acme::{Account, AccountCredentials, LetsEncrypt, NewAccount};
use log::info;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

static ACCOUNT: OnceLock<Account> = OnceLock::new();

pub async fn get_account(file: &str) -> Result<&'static Account, Box<dyn std::error::Error>> {
    if let Some(account) = ACCOUNT.get() {
        return Ok(account);
    }
    if let Some(credentials) = load_credentials(file) {
        let acc_builder = Account::builder()?;
        let account = acc_builder.from_credentials(credentials).await?;
        let _ = ACCOUNT.set(account);
        info!("Loaded existing ACME account");
    } else {
        info!("No existing credentials found, creating new account");
        create_account(file).await?;
    }

    ACCOUNT.get().ok_or("Failed to initialize account".into())
}

async fn create_account(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let new_account = NewAccount {
        contact: &[],
        terms_of_service_agreed: true,
        only_return_existing: false,
    };
    let acc_builder = Account::builder()?;
    let (account, credentials) = acc_builder.create(&new_account, LetsEncrypt::Production.url().to_string(), None).await?;
    // let (account, credentials) = acc_builder.create(&new_account, LetsEncrypt::Staging.url().to_string(), None).await?;
    info!("Account created: {:?}", account.id());
    save_credentials(&credentials, file)?;
    let _ = ACCOUNT.set(account);
    Ok(())
}

pub async fn load_or_create(file: &str) -> Result<String, Box<dyn std::error::Error>> {
    let account = get_account(file).await?;
    Ok(account.id().to_string() + "\n")
}

fn save_credentials(credentials: &AccountCredentials, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(credentials)?;
    fs::write(file, json)?;
    info!("ACME credentials saved to {}", file);
    Ok(())
}
fn load_credentials(file: &str) -> Option<AccountCredentials> {
    if !Path::new(file).exists() {
        return None;
    }
    let json = fs::read_to_string(file).ok()?;
    serde_json::from_str(&json).ok()
}
