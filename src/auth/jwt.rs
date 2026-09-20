use crate::auth::types::AuthValidator;
use crate::utils::jwt::{check_jwt, JWT_TOKEN};
use pingora_proxy::Session;
use std::collections::HashMap;
use urlencoding::decode;

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
