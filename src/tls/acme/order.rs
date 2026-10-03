use crate::tls::acme::account::get_account;
use crate::tls::acme::types::get_provider;
use crate::utils::lazylock::CHALLENGES;
use crate::utils::lazylock::DOMAINS;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use instant_acme::{ChallengeType, Identifier, NewOrder, RetryPolicy};
use log::{error, info};
use pingora::prelude::sleep;
use rcgen::{CertificateParams, DistinguishedName, KeyPair};
use sha2::{Digest, Sha256};
use std::fs;
use std::time::Duration;
use x509_parser::prelude::*;

static DNS: bool = true;

pub async fn refresh_order(certs_dir: String, autoconf_dir: String, provider: Option<String>) {
    let credsfile = autoconf_dir + "/acme_credentials.json";
    loop {
        for item in DOMAINS.iter() {
            let _what = order(item.key(), credsfile.as_str(), certs_dir.clone(), provider.clone()).await;
        }
        sleep(Duration::from_secs(12 * 3600)).await;
    }
}
pub async fn order(domain: &str, credsfile: &str, certs_dir: String, provider: Option<String>) -> Result<String, Box<dyn std::error::Error>> {
    let crt = certs_dir.clone() + "/" + domain + ".crt";
    let key = certs_dir.clone() + "/" + domain + ".key";

    if DOMAINS.get(domain).is_none() {
        DOMAINS.insert(domain.to_string(), true);
        let mut newlist: Vec<String> = Vec::new();
        for item in DOMAINS.iter() {
            newlist.push(item.key().to_string());
        }
        if let Ok(json_content) = serde_json::to_string_pretty(&newlist) {
            let autocfg_file = credsfile.replace("/acme_credentials.json", "/domains.json");
            if let Err(err) = std::fs::write(&autocfg_file, json_content) {
                error!("Error Updating domains for certificates: {} : {}", domain, err);
                return Err(Box::from(err));
            }
        }
    }
    if let Ok(expiry) = cert_expiry(crt.as_str()) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        if expiry > now + 30 * 24 * 3600 {
            // println!("Fresh certificate exists. Not renewing !");
            return Ok("Fresh certificate exists. Not renewing ! \n".to_string());
        }
    };

    let account = get_account(credsfile).await?;
    let mut order = account.new_order(&NewOrder::new(&[Identifier::Dns(domain.to_string())])).await?;

    let mut authorizations = order.authorizations();
    // let dns_provider = CloudflareProvider::new();

    let dns_provider = get_provider();

    while let Some(auth) = authorizations.next().await {
        let mut auth = auth?;
        let domain = auth.identifier().to_string();
        let challenge_type = if DNS { ChallengeType::Dns01 } else { ChallengeType::Http01 };

        let mut challenge_handle = auth.challenge(challenge_type).ok_or("No challenge found")?;
        let key_auth = challenge_handle.key_authorization();
        let key_auth_str = key_auth.as_str().to_string();

        if let Some(prov) = provider.clone() {
            println!("============================== {:?}", prov);
            let dns_value = calculate_dns_value(&key_auth_str);
            let clean_domain = domain.trim_start_matches("*.").to_string();
            let record_name = format!("_acme-challenge.{}", clean_domain);
            // Works identically for Cloudflare, Route53, or any future provider!
            match dns_provider.create_txt_record(&clean_domain, &record_name, &dns_value).await {
                Ok(record_id) => {
                    info!("Waiting 30 seconds for DNS propagation: {}", record_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                    challenge_handle.set_ready().await?;
                    let _ = dns_provider.delete_txt_record(&record_id).await;
                }
                Err(e) => {
                    eprintln!("Failed to create DNS record: {}", e);
                    return Err(e);
                }
            }
        } else {
            let token = key_auth_str.split('.').next().ok_or("invalid key authorization")?.to_string();
            CHALLENGES.write().unwrap().insert(token, key_auth_str);
            challenge_handle.set_ready().await?;
        }
    }

    let status = order.poll_ready(&RetryPolicy::default()).await?;
    info!("ACME poll_ready status: {:?}", status);

    let mut params = CertificateParams::new(vec![domain.to_owned()])?;
    params.distinguished_name = DistinguishedName::new();
    let private_key = KeyPair::generate()?;
    let signing_request = params.serialize_request(&private_key)?;
    let csr_der = signing_request.der();
    order.finalize_csr(csr_der).await?;

    // poll for certificate
    let cert_chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;
    CHALLENGES.write().unwrap().clear();
    let private_key_pem = private_key.serialize_pem();

    fs::write(crt, cert_chain_pem)?;
    fs::write(key, private_key_pem)?;
    Ok("Certificate is successfully generated \n".to_string())
}

fn cert_expiry(path: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let pem = fs::read(path)?;
    let (_, pem) = parse_x509_pem(&pem)?;
    let (_, cert) = parse_x509_certificate(&pem.contents)?;
    let expiry = cert.validity().not_after.timestamp() as u64;
    Ok(expiry)
}

fn calculate_dns_value(key_auth: &str) -> String {
    let hash = Sha256::digest(key_auth.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}
