use redis::{Client, aio::ConnectionManager};
use serde::{de::DeserializeOwned, Serialize};

pub struct CacheClient {
    manager: ConnectionManager,
}

impl CacheClient {
    pub async fn new(redis_url: &str) -> anyhow::Result<Self> {
        let client = Client::open(redis_url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { manager })
    }

    pub async fn get<T: DeserializeOwned>(&mut self, key: &str) -> anyhow::Result<Option<T>> {
        use redis::AsyncCommands;
        let val: Option<String> = self.manager.get(key).await?;
        Ok(val.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub async fn set<T: Serialize>(&mut self, key: &str, value: &T, ttl_secs: u64) -> anyhow::Result<()> {
        use redis::AsyncCommands;
        let serialized = serde_json::to_string(value)?;
        self.manager.set_ex(key, serialized, ttl_secs).await?;
        Ok(())
    }

    pub async fn del(&mut self, key: &str) -> anyhow::Result<()> {
        use redis::AsyncCommands;
        self.manager.del(key).await?;
        Ok(())
    }

    /// Store conversation memory
    pub fn conversation_key(conversation_id: &str) -> String {
        format!("conv:{}", conversation_id)
    }

    /// Store session token
    pub fn session_key(user_id: &str) -> String {
        format!("session:{}", user_id)
    }
}
