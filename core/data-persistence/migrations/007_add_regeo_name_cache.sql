-- T36: regeo 补名持久化缓存（坐标键 -> 名称；NULL 表示已查无名称，避免重复调用）。
-- 缓存键 = GCJ-02 经纬度 5 位小数；只做查询命中加速，不参与正式业务数据。
CREATE TABLE IF NOT EXISTS regeo_name_cache (
    cache_key  TEXT PRIMARY KEY,
    name       TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
