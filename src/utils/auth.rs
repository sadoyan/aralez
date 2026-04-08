use crate::utils::jwt::check_jwt;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pingora_proxy::Session;
use std::collections::HashMap;
use std::sync::Arc;
use urlencoding::decode;

trait AuthValidator {
    fn validate(&self, session: &Session) -> bool;
}
struct BasicAuth<'a>(&'a str);
struct ApiKeyAuth<'a>(&'a str);
struct JwtAuth<'a>(&'a str);

impl AuthValidator for BasicAuth<'_> {
    fn validate(&self, session: &Session) -> bool {
        if let Some(header) = session.get_header("authorization") {
            if let Some(h) = header.to_str().ok() {
                if let Some((_, val)) = h.split_once(' ') {
                    if let Some(decoded) = STANDARD.decode(val).ok() {
                        if let Some(decoded_str) = String::from_utf8(decoded).ok() {
                            return decoded_str == self.0;
                        }
                    }
                }
            }
        }
        false
    }
}

impl AuthValidator for ApiKeyAuth<'_> {
    fn validate(&self, session: &Session) -> bool {
        if let Some(header) = session.get_header("x-api-key") {
            if let Some(header) = header.to_str().ok() {
                return header == self.0;
            }
            // return header.to_str().ok().unwrap() == self.0;
        }
        false
    }
}

impl AuthValidator for JwtAuth<'_> {
    fn validate(&self, session: &Session) -> bool {
        let jwtsecret = self.0;
        if let Some(tok) = get_query_param(session, "araleztoken") {
            return check_jwt(tok.as_str(), jwtsecret);
        }
        if let Some(auth_header) = session.get_header("authorization") {
            if let Ok(header_str) = auth_header.to_str() {
                if let Some((scheme, token)) = header_str.split_once(' ') {
                    if scheme.eq_ignore_ascii_case("bearer") {
                        return check_jwt(token, jwtsecret);
                    }
                }
            }
        }
        false
    }
}
fn validate(auth: &dyn AuthValidator, session: &Session) -> bool {
    auth.validate(session)
}

// pub fn authenticate(c: &[Arc<str>], session: &Session) -> bool {
pub fn authenticate(auth_type: &Arc<str>, credentials: &Arc<str>, session: &Session) -> bool {
    match &*auth_type.clone() {
        "basic" => {
            let auth = BasicAuth(&*credentials.clone());
            validate(&auth, session)
        }
        "apikey" => {
            let auth = ApiKeyAuth(&*credentials.clone());
            validate(&auth, session)
        }
        "jwt" => {
            let auth = JwtAuth(&*credentials.clone());
            validate(&auth, session)
        }
        _ => {
            println!("Unsupported authentication mechanism : {}", auth_type);
            false
        }
    }
}

pub fn get_query_param(session: &Session, key: &str) -> Option<String> {
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

    params.get(key).map(|v| decode(v).ok()).flatten().map(|s| s.to_string())
}
