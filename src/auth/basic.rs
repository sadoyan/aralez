use crate::auth::types::AuthValidator;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pingora_proxy::Session;
use subtle::ConstantTimeEq;

pub struct BasicAuth<'a>(pub(crate) &'a str);
#[async_trait::async_trait]
impl AuthValidator for BasicAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        if let Some(header) = session.get_header("authorization") {
            if let Ok(h) = header.to_str() {
                if let Some((_, val)) = h.split_once(' ') {
                    if let Ok(decoded) = STANDARD.decode(val) {
                        if decoded.as_slice().ct_eq(self.0.as_bytes()).into() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}
