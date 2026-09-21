use crate::auth::types::AuthValidator;
use ahash::AHasher;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use moka::sync::Cache;
use moka::Expiry;
use pingora_proxy::Session;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};
use urlencoding::decode;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub master_key: String,
    pub owner: String,
    pub exp: u64,
    pub random: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Expired {
    exp: Option<u64>,
}

static JWT_VALIDATION: LazyLock<Validation> = LazyLock::new(|| Validation::new(Algorithm::HS256));

pub static JWT_TOKEN: LazyLock<Option<Arc<str>>> = LazyLock::new(|| match env::var("JWT_KEY") {
    Ok(key) if !key.is_empty() => Some(Arc::from(key.as_str())),
    _ => None,
});

static JWT_CACHE: LazyLock<Cache<u64, u64>> = LazyLock::new(|| Cache::builder().max_capacity(100_000).expire_after(JwtExpiry).build());
struct JwtExpiry;
impl Expiry<u64, u64> for JwtExpiry {
    fn expire_after_create(&self, _key: &u64, value: &u64, _current_time: Instant) -> Option<Duration> {
        let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        if *value > now {
            Some(Duration::from_secs(value - now))
        } else {
            Some(Duration::ZERO)
        }
    }
}
pub struct JwtAuth();
#[async_trait::async_trait]
impl AuthValidator for JwtAuth {
    async fn validate(&self, session: &mut Session) -> bool {
        if let Some(jwtsecret) = JWT_TOKEN.clone() {
            if let Some(tok) = get_query_param(session, "araleztoken") {
                return check_jwt(tok.as_str(), jwtsecret.as_ref());
            }
            if let Some(auth_header) = session.get_header("authorization") {
                if let Ok(header_str) = auth_header.to_str() {
                    if let Some((scheme, token)) = header_str.split_once(' ') {
                        if scheme.eq_ignore_ascii_case("bearer") {
                            return check_jwt(token, jwtsecret.as_ref());
                        }
                    }
                }
            }
        }
        false
    }
}

fn get_query_param(session: &mut Session, key: &str) -> Option<String> {
    let query = session.req_header().uri.query()?;

    let params: HashMap<_, _> = query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?;
            let v = parts.next().unwrap_or(""); // Some params might have no value
            Some((k, v))
        })
        .collect();
    params.get(key).and_then(|v| decode(v).ok()).map(|s| s.to_string())
}

pub fn check_jwt(token: &str, secret: &str) -> bool {
    let key = hash_token(token, secret);
    let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    if let Some(exp) = JWT_CACHE.get(&key) {
        if exp < now {
            return false;
        }
        return true;
    }
    match is_expired(token, now) {
        Ok(true) => return false,
        Ok(false) => {}
        Err(_) => return false,
    }

    match jsonwebtoken::decode::<Claims>(token, &DecodingKey::from_secret(secret.as_ref()), &JWT_VALIDATION) {
        Ok(data) => {
            let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            if data.claims.exp > now {
                JWT_CACHE.insert(key, data.claims.exp);
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

fn is_expired(token: &str, now: u64) -> Result<bool, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("Invalid JWT format".into());
    }
    let decoded = URL_SAFE_NO_PAD.decode(parts[1])?;
    let claims: Expired = serde_json::from_slice(&decoded)?;
    if let Some(exp) = claims.exp {
        Ok(exp < now)
    } else {
        Ok(true)
    }
}

fn hash_token(token: &str, secret: &str) -> u64 {
    let mut hasher = AHasher::default();
    token.hash(&mut hasher);
    secret.hash(&mut hasher);
    hasher.finish()
}
