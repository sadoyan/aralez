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
    let crt = format!("{}/{}.crt", certs_dir, domain);
    let key = format!("{}/{}.key", certs_dir, domain);

    if DOMAINS.get(domain).is_none() {
        DOMAINS.insert(domain.to_string(), true);
        let newlist: Vec<String> = DOMAINS.iter().map(|item| item.key().to_string()).collect();
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
            return Ok("Fresh certificate exists. Not renewing ! \n".to_string());
        }
    };

    let account = get_account(credsfile).await?;
    let mut order = account.new_order(&NewOrder::new(&[Identifier::Dns(domain.to_string())])).await?;

    let mut authorizations = order.authorizations();

    while let Some(auth) = authorizations.next().await {
        let mut auth = auth?;
        let domain_name = auth.identifier().to_string();
        let (challenge_type, is_dns) = match &provider {
            Some(_) => (ChallengeType::Dns01, true),
            None => (ChallengeType::Http01, false),
        };

        let mut challenge_handle = auth.challenge(challenge_type).ok_or("Requested challenge type not offered by ACME server")?;
        let key_auth = challenge_handle.key_authorization();
        let key_auth_str = key_auth.as_str().to_string();

        if is_dns {
            let prov = provider.as_ref().unwrap();
            let dns_provider = get_provider(prov.clone());
            let dns_value = calculate_dns_value(&key_auth_str);
            let clean_domain = domain_name.trim_start_matches("*.").to_string();
            let record_name = format!("_acme-challenge.{}", clean_domain);

            match dns_provider.create_txt_record(&clean_domain, &record_name, &dns_value).await {
                Ok(record_id) => {
                    let ready_res = challenge_handle.set_ready().await;
                    let _ = dns_provider.delete_txt_record(&record_id, &record_name).await;
                    ready_res?;
                }
                Err(e) => {
                    eprintln!("Failed to create DNS record: {}", e);
                    return Err(e);
                }
            }
        } else {
            let token = key_auth_str.split('.').next().ok_or("Invalid key authorization")?.to_string();
            CHALLENGES.write().unwrap().insert(token, key_auth_str);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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
