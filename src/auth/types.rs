use crate::auth::apikey::ApiKeyAuth;
use crate::auth::basic::BasicAuth;
use crate::auth::forward::ForwardAuth;
use crate::auth::jwt::JwtAuth;
use crate::utils::structs::InnerAuth;
use pingora_proxy::Session;

#[async_trait::async_trait]
pub trait AuthValidator {
    async fn validate(&self, session: &mut Session) -> bool;
}
pub async fn authenticate(auth: &InnerAuth, session: &mut Session) -> bool {
    match &*auth.auth_type {
        "basic" => BasicAuth(&*auth.auth_cred).validate(session).await,
        "apikey" => ApiKeyAuth(&*auth.auth_cred).validate(session).await,
        "jwt" => JwtAuth().validate(session).await,
        "forward" => ForwardAuth(&*auth.auth_cred).validate(session).await,
        _ => {
            log::warn!("Unsupported authentication mechanism : {}", &*auth.auth_type);
            false
        }
    }
}
