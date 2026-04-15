use ahash::AHasher;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Claims {
    pub(crate) user: String,
    pub(crate) exp: u64,
}

static JWT_CACHE: LazyLock<Cache<u64, bool>> = LazyLock::new(|| Cache::builder().max_capacity(100_000).time_to_live(std::time::Duration::from_secs(60)).build());
static JWT_VALIDATION: LazyLock<Validation> = LazyLock::new(|| Validation::new(Algorithm::HS256));

/*
pub fn check_jwt(input: &str, secret: &str) -> bool {
    let validation = Validation::new(Algorithm::HS256);
    let token_data = decode::<Claims>(&input, &DecodingKey::from_secret(secret.as_ref()), &validation);
    token_data.is_ok()
}
*/

pub fn check_jwt(token: &str, secret: &str) -> bool {
    let key = hash_token(token, secret);
    if let Some(v) = JWT_CACHE.get(&key) {
        return v;
    }
    let result = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_ref()), &JWT_VALIDATION).is_ok();
    if result {
        JWT_CACHE.insert(key, true);
    }
    result
}

fn hash_token(token: &str, secret: &str) -> u64 {
    let mut hasher = AHasher::default();
    token.hash(&mut hasher);
    secret.hash(&mut hasher);
    hasher.finish()
}
