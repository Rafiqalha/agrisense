use serde::Serialize;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;

#[derive(Serialize)]
pub struct RagRecord {
    pub doc_id: String,
    pub text_context: String,
    pub metadata: Metadata,
}

#[derive(Serialize)]
pub struct Metadata {
    pub title: String,
    pub source_url: String,
    pub category: String,
    pub publish_date: String,
}

pub struct StorageManager {
    writer: Arc<Mutex<BufWriter<tokio::fs::File>>>,
}

impl StorageManager {
    pub async fn new(
        path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(path)
            .await?;
        Ok(Self {
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
        })
    }

    pub async fn append(
        &self,
        record: &RagRecord,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        let mut guard = self.writer.lock().await;
        guard.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut guard = self.writer.lock().await;
        guard.flush().await?;
        Ok(())
    }
}
