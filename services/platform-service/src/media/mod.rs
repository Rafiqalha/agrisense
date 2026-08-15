//! Media Module — upload, resize, GCS storage, OCR pipeline
//!
//! Responsibilities:
//!   - Upload media to GCS
//!   - Generate thumbnails
//!   - Download from WhatsApp Media API
//!   - OCR pipeline entry point
//!
//! When extracted: becomes media-service (port 3012)

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .route("/upload", axum::routing::post(|| async { "TODO: upload" }))
        .route("/:id", axum::routing::get(|| async { "TODO: get media" }))
}
