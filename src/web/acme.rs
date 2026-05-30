use crate::tls::acme::order::CHALLENGES;
use crate::tls::acme::{account, order};
use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;

#[allow(clippy::needless_return)]
pub async fn acme_create(State(state): State<crate::web::webserver::AppState>) -> impl IntoResponse {
    match account::load_or_create(state.cert_creds.as_str()).await {
        Ok(txt) => {
            return Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain")
                .body(Body::from(txt))
                .unwrap()
        }
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Failed to create account: {}", e)))
                .unwrap()
        }
    };
}
#[allow(clippy::needless_return)]
pub async fn acme_order(State(state): State<crate::web::webserver::AppState>, axum::extract::Path(domain): axum::extract::Path<String>) -> impl IntoResponse {
    let domain_clean = domain.trim_matches('/');
    match order::order(domain_clean, state.cert_creds.as_str(), state.certs_dir).await {
        Ok(txt) => {
            return Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain")
                .body(Body::from(txt))
                .unwrap()
        }
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Failed to order a certificate: {}", e)))
                .unwrap()
        }
    };
}
pub async fn http01_challenge(axum::extract::Path(token): axum::extract::Path<String>) -> impl IntoResponse {
    if let Ok(challenges) = CHALLENGES.read() {
        // for k in challenges.iter() {
        //     println!("   ==> {} : {}", k.0, k.1);
        // }

        if let Some(key_authorization) = challenges.get(&token) {
            return Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain")
                .body(Body::from(key_authorization.clone()))
                .unwrap();
        }
    }
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain")
        .body(Body::from("Not found"))
        .unwrap()
}
