use dashmap::DashMap;
use log::error;
use pingora::tls::ssl::{select_next_proto, AlpnError, NameType, SniError, SslAlert, SslContext, SslFiletype, SslMethod, SslRef};
use rustls_pemfile::{read_one, Item};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
// use tokio::time::Instant;
use x509_parser::extensions::GeneralName;
use x509_parser::nom::Err as NomErr;
use x509_parser::prelude::*;

#[derive(Clone, Deserialize, Debug)]
pub struct CertificateConfig {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug)]
struct CertificateInfo {
    common_names: Vec<String>,
    alt_names: Vec<String>,
    ssl_context: SslContext,
    #[allow(dead_code)]
    cert_path: String, // Only used for logging
    #[allow(dead_code)]
    key_path: String, // Only used for logging
}

#[derive(Debug)]
pub struct Certificates {
    configs: Vec<CertificateInfo>,
    name_map: DashMap<String, SslContext>,
    pub default_cert_path: String,
    pub default_key_path: String,
}

impl Certificates {
    pub fn new(configs: &Vec<CertificateConfig>) -> Option<Self> {
        let default_cert = configs.first().expect("At least one TLS certificate required");
        let mut cert_infos = Vec::new();
        let name_map: DashMap<String, SslContext> = DashMap::new();
        for config in configs {
            let cert_info = load_cert_info(&config.cert_path, &config.key_path);
            match cert_info {
                Some(cert) => {
                    for name in &cert.common_names {
                        name_map.insert(name.clone(), cert.ssl_context.clone());
                    }
                    for name in &cert.alt_names {
                        name_map.insert(name.clone(), cert.ssl_context.clone());
                    }

                    cert_infos.push(cert)
                }
                None => {
                    error!("Unable to load certificate info | public: {}, private: {}", &config.cert_path, &config.key_path);
                    return None;
                }
            }
        }
        Some(Self {
            name_map: name_map,
            configs: cert_infos,
            default_cert_path: default_cert.cert_path.clone(),
            default_key_path: default_cert.key_path.clone(),
        })
    }

    fn find_ssl_context(&self, server_name: &str) -> Option<SslContext> {
        if let Some(ctx) = self.name_map.get(server_name) {
            return Some(ctx.clone());
        }
        for config in &self.configs {
            for name in &config.common_names {
                if name.starts_with("*.") && server_name.ends_with(&name[1..]) {
                    return Some(config.ssl_context.clone());
                }
            }
            for name in &config.alt_names {
                if name.starts_with("*.") && server_name.ends_with(&name[1..]) {
                    return Some(config.ssl_context.clone());
                }
            }
        }
        None
    }

    pub fn server_name_callback(&self, ssl_ref: &mut SslRef, ssl_alert: &mut SslAlert) -> Result<(), SniError> {
        let server_name = ssl_ref.servername(NameType::HOST_NAME);
        log::debug!("TLS connect: server_name = {:?}, ssl_ref = {:?}, ssl_alert = {:?}", server_name, ssl_ref, ssl_alert);
        // let start_time = Instant::now();
        if let Some(name) = server_name {
            match self.find_ssl_context(name) {
                Some(ctx) => {
                    ssl_ref.set_ssl_context(&*ctx).map_err(|_| SniError::ALERT_FATAL)?;
                }
                None => {
                    log::debug!("No matching server name found");
                }
            }
        }
        // println!("Context  ==> {:?} <==", start_time.elapsed());
        Ok(())
    }
}

fn load_cert_info(cert_path: &str, key_path: &str) -> Option<CertificateInfo> {
    let mut common_names = HashSet::new();
    let mut alt_names = HashSet::new();

    let file = File::open(cert_path);
    match file {
        Err(e) => {
            log::error!("Failed to open certificate file: {:?}", e);
            return None;
        }
        Ok(file) => {
            let mut reader = BufReader::new(file);
            match read_one(&mut reader) {
                Err(e) => {
                    log::error!("Failed to decode PEM from certificate file: {:?}", e);
                    return None;
                }
                Ok(leaf) => match leaf {
                    Some(Item::X509Certificate(cert)) => match X509Certificate::from_der(&cert) {
                        Err(NomErr::Error(e)) | Err(NomErr::Failure(e)) => {
                            log::error!("Failed to parse certificate: {:?}", e);
                            return None;
                        }
                        Err(_) => {
                            log::error!("Unknown error while parsing certificate");
                            return None;
                        }
                        Ok((_, x509)) => {
                            let subject = x509.subject();
                            for attr in subject.iter_common_name() {
                                if let Ok(cn) = attr.as_str() {
                                    common_names.insert(cn.to_string());
                                }
                            }

                            if let Ok(Some(san)) = x509.subject_alternative_name() {
                                for name in san.value.general_names.iter() {
                                    if let GeneralName::DNSName(dns) = name {
                                        let dns_string = dns.to_string();
                                        if !common_names.contains(&dns_string) {
                                            alt_names.insert(dns_string);
                                        }
                                    }
                                }
                            }
                        }
                    },
                    _ => {
                        log::error!("Failed to read certificate");
                        return None;
                    }
                },
            }
        }
    }

    if let Ok(ssl_context) = create_ssl_context(cert_path, key_path) {
        Some(CertificateInfo {
            cert_path: cert_path.to_string(),
            key_path: key_path.to_string(),
            common_names: common_names.into_iter().collect(),
            alt_names: alt_names.into_iter().collect(),
            ssl_context,
        })
    } else {
        log::error!("Failed to create SSL context from cert paths");
        None
    }
}

fn create_ssl_context(cert_path: &str, key_path: &str) -> Result<SslContext, Box<dyn std::error::Error>> {
    let mut ctx = SslContext::builder(SslMethod::tls())?;
    ctx.set_certificate_chain_file(cert_path)?;
    ctx.set_private_key_file(key_path, SslFiletype::PEM)?;
    ctx.set_alpn_select_callback(prefer_h2);
    let built = ctx.build();
    Ok(built)
}

pub fn prefer_h2<'a>(_ssl: &mut SslRef, alpn_in: &'a [u8]) -> Result<&'a [u8], AlpnError> {
    match select_next_proto("\x02h2\x08http/1.1".as_bytes(), alpn_in) {
        Some(p) => Ok(p),
        _ => Err(AlpnError::NOACK),
    }
}
