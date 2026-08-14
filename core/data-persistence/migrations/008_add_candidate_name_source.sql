-- 补名来源：候选投影保存名称来源（OSM/高德/缓存/仍未命名/失败）。
-- 只作为方案级候选的派生事实，不写入原始观测（原始证据仍是来源 payload）。
ALTER TABLE candidate_projections ADD COLUMN name_source TEXT NOT NULL DEFAULT 'unnamed';
