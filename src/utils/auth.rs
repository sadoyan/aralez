use crate::utils::jwt::check_jwt;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pingora_proxy::Session;

trait AuthValidator {
    fn validate(&self, session: &Session) -> bool;
}
struct BasicAuth<'a>(&'a str);
struct ApiKeyAuth<'a>(&'a str);
struct JwtAuth<'a>(&'a str);

impl AuthValidator for BasicAuth<'_> {
    fn validate(&self, session: &Session) -> bool {
        if let Some(header) = session.get_header("authorization") {
            if let Some((_, val)) = header.to_str().ok().unwrap().split_once(' ') {
                let decoded = STANDARD.decode(val).ok().unwrap();
                let decoded_str = String::from_utf8(decoded).ok().unwrap();
                return decoded_str == self.0;
            }
        }
        false
    }
}

impl AuthValidator for ApiKeyAuth<'_> {
    fn validate(&self, session: &Session) -> bool {
        if let Some(header) = session.get_header("x-api-key") {
            return header.to_str().ok().unwrap() == self.0;
        }
        false
    }
}

impl AuthValidator for JwtAuth<'_> {
    fn validate(&self, session: &Session) -> bool {
        let jwtsecret = self.0;
        if let Some(header) = session.get_header("x-jwt-token") {
            let tok = header.to_str().ok().unwrap();
            return check_jwt(tok, jwtsecret);
        }
        false
    }
}
fn validate(auth: &dyn AuthValidator, session: &Session) -> bool {
    auth.validate(session)
}

pub fn authenticate(c: &[String], session: &Session) -> bool {
    match c[0].as_str() {
        "basic" => {
            let auth = BasicAuth(c[1].as_str().into());
            validate(&auth, session)
        }
        "apikey" => {
            let auth = ApiKeyAuth(c[1].as_str().into());
            validate(&auth, session)
        }
        "jwt" => {
            let auth = JwtAuth(c[1].as_str().into());
            validate(&auth, session)
        }
        _ => {
            println!("Unsupported authentication mechanism : {}", c[0]);
            false
        }
    }
}
