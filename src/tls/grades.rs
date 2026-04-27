use log::{info, warn};
use pingora::tls::ssl::{select_next_proto, AlpnError, SslRef, SslVersion};
use pingora_core::listeners::tls::TlsSettings;

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
            // let _ = tls_settings.set_ciphersuites(CIPHERS.high);
            let _ = tls_settings.set_cipher_list(CIPHERS.high);
            info!("TLS grade: {:?}, => HIGH", tls_settings.options());
        }
        Some(TlsGrade::MEDIUM) => {
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::TLS1));
            let _ = tls_settings.set_cipher_list(CIPHERS.medium);
            // let _ = tls_settings.set_ciphersuites(CIPHERS.medium);
            let _ = tls_settings.set_cipher_list(CIPHERS.medium);
            info!("TLS grade: {:?}, => MEDIUM", tls_settings.options());
        }
        Some(TlsGrade::LEGACY) => {
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::SSL3));
            let _ = tls_settings.set_cipher_list(CIPHERS.legacy);
            // let _ = tls_settings.set_ciphersuites(CIPHERS.legacy);
            let _ = tls_settings.set_cipher_list(CIPHERS.legacy);
            warn!("TLS grade: {:?}, => UNSAFE", tls_settings.options());
        }
        None => {
            // Defaults to MEDIUM
            let _ = tls_settings.set_min_proto_version(Some(SslVersion::TLS1));
            let _ = tls_settings.set_cipher_list(CIPHERS.medium);
            // let _ = tls_settings.set_ciphersuites(CIPHERS.medium);
            let _ = tls_settings.set_cipher_list(CIPHERS.medium);
            warn!("TLS grade is not detected defaulting top MEDIUM");
        }
    }
}
