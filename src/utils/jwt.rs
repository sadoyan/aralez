use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Claims {
    pub(crate) user: String,
    pub(crate) exp: u64,
}
pub fn check_jwt(input: &str, secret: &str) -> bool {
    let validation = Validation::new(Algorithm::HS256);
    let token_data = decode::<Claims>(&input, &DecodingKey::from_secret(secret.as_ref()), &validation);
    match token_data {
        Ok(_) => true,
        Err(_) => false,
    }
}
