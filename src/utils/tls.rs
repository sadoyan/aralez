use dashmap::DashMap;
use log::{error, info, warn};
use pingora::tls::ssl::{select_next_proto, AlpnError, NameType, SniError, SslAlert, SslContext, SslFiletype, SslMethod, SslRef, SslVersion};
use pingora_core::listeners::tls::TlsSettings;
use rustls_pemfile::{read_one, Item};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
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
    pub fn new(configs: &Vec<CertificateConfig>, _grade: &str) -> Option<Self> {
        let default_cert = configs.first().expect("At least one TLS certificate required");
        let mut cert_infos = Vec::new();
        let name_map: DashMap<String, SslContext> = DashMap::new();
        for config in configs {
            let cert_info = load_cert_info(&config.cert_path, &config.key_path, _grade);
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

fn load_cert_info(cert_path: &str, key_path: &str, _grade: &str) -> Option<CertificateInfo> {
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

#[derive(Debug)]
pub struct CipherSuite {
    pub high: &'static str,
    pub medium: &'static str,
    pub legacy: &'static str,
}
const CIPHERS: CipherSuite = CipherSuite {
    high: "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305",
    medium: "ECDHE-RSA-AES128-SHA:ECDHE-ECDSA-AES128-SHA:AES128-GCM-SHA256",
    legacy: "ALL:!ADH:!LOW:!EXP:!MD5:@STRENGTH",
};

#[derive(Debug)]
pub enum TlsGrade {
    HIGH,
    MEDIUM,
    LEGACY,
}

impl TlsGrade {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "high" => Some(TlsGrade::HIGH),
            "medium" => Some(TlsGrade::MEDIUM),
            "unsafe" => Some(TlsGrade::LEGACY),
            _ => None,
        }
    }
}
pub fn prefer_h2<'a>(_ssl: &mut SslRef, alpn_in: &'a [u8]) -> Result<&'a [u8], AlpnError> {
    match select_next_proto("\x02h2\x08http/1.1".as_bytes(), alpn_in) {
        Some(p) => Ok(p),
        _ => Err(AlpnError::NOACK),
    }
}

pub fn set_tsl_grade(tls_settings: &mut TlsSettings, grade: &str) {
    let config_grade = TlsGrade::from_str(grade);
    match config_grade {
        Some(TlsGrade::HIGH) => {
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::TLS1_2));
            // let _ = tls_settings.set_max_proto_version(Some(SslVersion::TLS1_3));
            let _ = tls_settings.set_cipher_list(CIPHERS.high);
            let _ = tls_settings.set_ciphersuites(CIPHERS.high);
            info!("TLS grade: {:?}, => HIGH", tls_settings.options());
        }
        Some(TlsGrade::MEDIUM) => {
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::TLS1));
            let _ = tls_settings.set_cipher_list(CIPHERS.medium);
            let _ = tls_settings.set_ciphersuites(CIPHERS.medium);
            info!("TLS grade: {:?}, => MEDIUM", tls_settings.options());
        }
        Some(TlsGrade::LEGACY) => {
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::SSL3));
            let _ = tls_settings.set_cipher_list(CIPHERS.legacy);
            let _ = tls_settings.set_ciphersuites(CIPHERS.legacy);
            warn!("TLS grade: {:?}, => UNSAFE", tls_settings.options());
        }
        None => {
            // Defaults to MEDIUM
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::TLS1));
            let _ = tls_settings.set_cipher_list(CIPHERS.medium);
            let _ = tls_settings.set_ciphersuites(CIPHERS.medium);
            warn!("TLS grade is not detected defaulting top MEDIUM");
        }
    }
}
