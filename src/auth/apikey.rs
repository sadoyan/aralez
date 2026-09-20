use crate::auth::types::AuthValidator;
use pingora_proxy::Session;
use subtle::ConstantTimeEq;

pub struct ApiKeyAuth<'a>(pub(crate) &'a str);
#[async_trait::async_trait]
impl AuthValidator for ApiKeyAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        if let Some(header) = session.get_header("x-api-key") {
            if let Ok(h) = header.to_str() {
                return h.as_bytes().ct_eq(self.0.as_bytes()).into();
            }
        }
        false
    }
}
