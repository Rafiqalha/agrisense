//! Identity Module — owns: identity.users, identity.sessions
//!
//! Responsibilities:
//!   - Farmer registration
//!   - Phone number verification
//!   - JWT token generation/validation
//!   - Session management
//!   - Role-based access control
//!
//! When extracted: becomes identity-service (port 3010)

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .route(
            "/register",
            axum::routing::post(|| async { "TODO: register" }),
        )
        .route("/login", axum::routing::post(|| async { "TODO: login" }))
        .route(
            "/verify",
            axum::routing::post(|| async { "TODO: verify phone" }),
        )
        .route(
            "/profile/:id",
            axum::routing::get(|| async { "TODO: get profile" }),
        )
}
