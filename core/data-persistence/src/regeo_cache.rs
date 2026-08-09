//! T36: regeo 补名持久化缓存（SQLite）。
//!
//! 补名缓存从“会话级”改为“持久化”：同坐标重复采集不再重复调用高德 regeo。
//! 缓存键为 GCJ-02 经纬度（5 位小数，约 1 米粒度）；`name` 为 NULL 表示
//! “已查过但无名称”，同样写缓存避免重复调用。
//!
//! 本类型持有独立连接（`Mutex` 包住），供 F4 有界并发补名的多个 worker
//! 线程共享；rusqlite 仍只在 B2 出现（deny bans + clippy 禁用表豁免方）。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::Result;
use crate::migrations;

/// regeo 补名缓存的公开存储接口（B2 对外能力，F4 消费）。
pub trait RegeoNameCacheApi: Send + Sync {
    /// 读取缓存：`Ok(Some(Some(name)))` 命中名称；`Ok(Some(None))` 命中“已查无名称”；
    /// `Ok(None)` 未缓存；`Err` 存储不可用（调用方降级为不缓存，不阻断补名）。
    fn get_regeo_name(&self, cache_key: &str) -> Result<Option<Option<String>>>;

    /// 写入缓存：`name` 为 `None` 时记录“已查无名称”。
    fn put_regeo_name(&self, cache_key: &str, name: Option<&str>) -> Result<()>;
}

/// SQLite regeo 补名缓存：独立连接 + 内部互斥，可跨线程共享。
pub struct RegeoNameCache {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for RegeoNameCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegeoNameCache").finish_non_exhaustive()
    }
}

impl RegeoNameCache {
    /// 打开（或创建）指定路径的缓存数据库，并迁移到最新 schema。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// 打开内存缓存（测试与降级兜底）。
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        migrations::run_migrations(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl RegeoNameCacheApi for RegeoNameCache {
    fn get_regeo_name(&self, cache_key: &str) -> Result<Option<Option<String>>> {
        use rusqlite::OptionalExtension;
        let conn = self.conn.lock().expect("regeo cache connection lock");
        let row: Option<Option<String>> = conn
            .query_row(
                "SELECT name FROM regeo_name_cache WHERE cache_key = ?1",
                [cache_key],
                |row| row.get(0),
            )
            .optional()?;
        // 无行 -> Ok(None)；有行但 name 为 NULL（已查无名称）-> Ok(Some(None))
        Ok(row)
    }

    fn put_regeo_name(&self, cache_key: &str, name: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().expect("regeo cache connection lock");
        conn.execute(
            "INSERT INTO regeo_name_cache (cache_key, name) VALUES (?1, ?2)
             ON CONFLICT(cache_key) DO UPDATE SET name = excluded.name, updated_at = datetime('now')",
            rusqlite::params![cache_key, name],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip_persists_name_and_miss() {
        let cache = RegeoNameCache::open_in_memory().unwrap();
        assert_eq!(cache.get_regeo_name("121.42800,31.02800").unwrap(), None);

        cache
            .put_regeo_name("121.42800,31.02800", Some("第一教学楼"))
            .unwrap();
        assert_eq!(
            cache
                .get_regeo_name("121.42800,31.02800")
                .unwrap()
                .flatten()
                .as_deref(),
            Some("第一教学楼")
        );

        cache.put_regeo_name("121.42801,31.02801", None).unwrap();
        assert_eq!(
            cache.get_regeo_name("121.42801,31.02801").unwrap(),
            Some(None),
            "已查无名称也必须缓存，避免重复调用"
        );
    }

    #[test]
    fn update_overwrites_previous_name() {
        let cache = RegeoNameCache::open_in_memory().unwrap();
        cache.put_regeo_name("k", Some("旧名")).unwrap();
        cache.put_regeo_name("k", Some("新名")).unwrap();
        assert_eq!(
            cache.get_regeo_name("k").unwrap().flatten().as_deref(),
            Some("新名")
        );
    }
}
