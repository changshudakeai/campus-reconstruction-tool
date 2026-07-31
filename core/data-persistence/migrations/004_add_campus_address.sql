-- 校区地址列（ADR-0006：最近使用记录卡片同时展示校区名称和地址）
ALTER TABLE campuses ADD COLUMN address TEXT NOT NULL DEFAULT '';
