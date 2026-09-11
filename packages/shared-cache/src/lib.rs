use redis::{aio::ConnectionManager, Client};
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

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<Option<T>> {
        use redis::AsyncCommands;
        let mut manager = self.manager.clone();
        let val: Option<String> = manager.get(key).await?;
        Ok(val.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> anyhow::Result<()> {
        use redis::AsyncCommands;
        let mut manager = self.manager.clone();
        let serialized = serde_json::to_string(value)?;
        let _: () = manager.set_ex(key, serialized, ttl_secs).await?;
        Ok(())
    }

    pub async fn del(&self, key: &str) -> anyhow::Result<()> {
        use redis::AsyncCommands;
        let mut manager = self.manager.clone();
        let _: () = manager.del(key).await?;
        Ok(())
    }

    pub async fn is_ready(&self) -> bool {
        let mut manager = self.manager.clone();
        redis::cmd("PING")
            .query_async::<String>(&mut manager)
            .await
            .is_ok()
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
