# T05: Campus Search & Anchor Persistence（校区搜索与锚点持久化）

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

**工单编号**: T05  
**优先级**: ⭐⭐⭐⭐ (直接影响首次采集体验)  
**关联**: ADR-0004, ADR-0007, ADR-0008, handoff-2026-07-28-t25-complete.md  
**状态**: `completed`（2026-07-28 随 T05 commit 4c5a647 落地：校区锚点持久化）

---

## 🎯 问题描述

### 当前痛点

1. **校区搜索时无法保存位置偏好**：
   - F1 高德地图选校区时点击"确认"
   - 只保存了校区名称（如"北京大学"）
   - **未保存中心坐标作为锚点**（anchor_lng/anchor_lat）
   - 下次启动时高德地图仍显示北京（默认值）

2. **用户体验影响**：
   - 新用户每次都要重新缩放地图找位置
   - 边界绘制时锚点偏移数百米（高斯偏移 + GCJ-02 不匹配）
   - OSM 返回的 Polygon 在错误区域（如用户选了"清华大学",OSM 却返回北京其他区）

3. **技术限制**:
   - campuses 表缺少 `anchor_lng`、`anchor_lat` 列（schema draft v1.sql）
   - F1 global-settings 无持久化机制记录锚点
   - B3 gaode-client 仅有搜索接口，无存储逻辑

---

## ✅ 验收标准

### 功能验收

1. **campuses 表 schema 变更**:
   ```sql
   ALTER TABLE campuses ADD COLUMN anchor_lng REAL NOT NULL DEFAULT 116.397;
   ALTER TABLE campuses ADD COLUMN anchor_lat REAL NOT NULL DEFAULT 39.916;
   -- index? optional for now
   ```

2. **F1 UI 交互增强**:
   - 搜索框输入校区名后显示列表
   - 点击某个结果 → 高德地图飞入该校区范围
   - 地图上可拖拽 marker 或调整视图
   - **"确认"按钮** 将以下信息落库：
     - campus_name（已有）
     - campus_poi_id（新）
     - **anchor_lng / anchor_lat** （新，来自地图中心坐标）
     - **created_at** / **updated_at** （自动更新）

3. **B3 gaode-client API 扩展**:
   ```rust
   pub struct CampusSearchResult {
       pub name: String,
       pub poi_id: String,
       pub center_lng: f64,   // ← 新增
       pub center_lat: f64,   // ← 新增
       pub bbox: Option<(f64, f64, f64, f64)>,
   }
   
   impl CampusPoiRecord {
       // 添加 new() method with anchor
       pub fn new(name: &str, poi_id: &str, lng: f64, lat: f64) -> Self;
   }
   ```

4. **B2 data-persistence**:
   - `Campus` 模型增加 `anchor_lng` / `anchor_lat` 字段
   - repository methods:
     ```rust
     pub fn create_campus(&mut self, name: &str, poi_id: &str, 
                         anchor_lng: f64, anchor_lat: f64) -> Result<CampusId>;
     pub fn update_anchor(&mut self, campus_id: CampusId, 
                         lng: f64, lat: f64) -> Result<()>;
     ```

5. **UI 验证**:
   - 完成 F1 配置后，再次打开应用
   - 高德地图自动定位到上次保存的校区位置
   - **不再出现空白的北京中心**

---

## 🔧 实施步骤

### Step 1: Schema 变更
```bash
# Create migration script

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）
cd New-branch-v2/core/data-persistence/migrations
touch 003_add_anchor_columns_to_campuses.sql

# Edit the file

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）
cat > 003_add_anchor_columns_to_campuses.sql << 'EOF'
-- MCRebuild V2 数据库迁移脚本 v3 — 校区锚点列
--
-- 依据：T05 需求 + ADR-0004(全局设置) + ADR-0007(方案隔离)
-- 版本号由迁移执行器统一写入 schema_migrations，本脚本不自行插入。

ALTER TABLE campuses ADD COLUMN anchor_lng REAL NOT NULL DEFAULT 116.397;
ALTER TABLE campuses ADD COLUMN anchor_lat REAL NOT NULL DEFAULT 39.916;
EOF
```

### Step 2: Domain 层改造 (`core/shared-domain-types`)
```rust
// Add to core/shared-domain-types/src/campus.rs

pub struct Campus {
    pub id: CampusId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub anchor_lng: f64,  // ← NEW
    pub anchor_lat: f64,  // ← NEW
}

impl From<CampusRow> for Campus {
    fn from(row: CampusRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            created_at: row.created_at.parse().unwrap(),
            updated_at: row.updated_at.parse().unwrap(),
            anchor_lng: row.anchor_lng,   // ← NEW
            anchor_lat: row.anchor_lat,   // ← NEW
        }
    }
}
```

### Step 3: DataPersistence 层 (`core/data-persistence`)
```rust
// core/data-persistence/src/repository/campus.rs

pub struct CampusRepository<'a> {
    db: &'a Database,
}

impl<'a> CampusRepository<'a> {
    pub fn create_with_anchor(
        &self, 
        name: &str, 
        poi_id: &str,
        anchor_lng: f64, 
        anchor_lat: f64,
    ) -> Result<CampusId> {
        let now = Utc::now().to_rfc3339();
        
        self.db.execute(
            "INSERT INTO campuses (name, poi_id, anchor_lng, anchor_lat, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![name, poi_id, anchor_lng, anchor_lat, now, now],
        )?;
        
        Ok(CampusId::new(self.db.last_insert_rowid()))
    }
    
    pub fn update_anchor(&self, campus_id: CampusId, lng: f64, lat: f64) -> Result<()> {
        self.db.execute(
            "UPDATE campuses SET anchor_lng = ?, anchor_lat = ?, updated_at = ?
             WHERE id = ?",
            params![lng, lat, Utc::now().to_rfc3339(), campus_id.as_u64()],
        )?;
        Ok(())
    }
}
```

### Step 4: Global Settings 模块 (`core/global-settings`)
```rust
// core/global-settings/src/settings_manager.rs

impl SettingsManager {
    pub fn select_campus(&mut self, campus_name: &str, poi_id: &str, 
                        anchor_lng: f64, anchor_lat: f64) -> Result<()> {
        let tx = self.db.transaction()?;
        
        // Insert into campuses
        let campus_id = tx.create_with_anchor(campus_name, poi_id, anchor_lng, anchor_lat)?;
        
        // Set first_run_completed = true (for user flow)
        tx.set_app_setting("first_run_completed", "true")?;
        
        // Set last_used_campus
        self.set_last_used_campus(campus_id);
        
        tx.commit()?;
        Ok(())
    }
}
```

### Step 5: Gaode Client 模块 (`core/gaode-client`)
```rust
// core/gaode-client/src/search_flow.rs

#[derive(Debug, Clone)]
pub struct CampusPoiRecord {
    pub name: String,
    pub poi_id: String,
    pub center_lng: f64,   // ← NEW
    pub center_lat: f64,   // ← NEW
    pub bbox: Option<BBox>,
}

impl CampusPoiRecord {
    pub fn new(name: &str, poi_id: &str, lng: f64, lat: f64) -> Self {
        Self {
            name: name.to_string(),
            poi_id: poi_id.to_string(),
            center_lng: lng,
            center_lat: lat,
            bbox: None,
        }
    }
}

// Search function enhancement
pub fn search_campus(client: &GaodeClient, query: &str) -> Result<Vec<CampusPoiRecord>> {
    let url = format!(
        "https://restapi.amap.com/v3/place/text?key={}&keywords={}",
        client.api_key(), query,
    );
    
    // Parse response and include center coordinates
    // See https://lbs.amap.com/api/webservice/guide/api/search
    Ok(search_results.into_iter()
        .map(|r| CampusPoiRecord::new(
            &r.name,
            &r.poiid,
            r.location.lng.parse().unwrap(),  // ← Extract from POI response
            r.location.lat.parse().unwrap(),  // ← Extract from POI response
        ))
        .collect()
    )
}
```

### Step 6: Desktop Shell (Slint UI) - F1
```slint
// apps/desktop/ui/f1_global_settings.slint

import { SlintInputField, SlintListView, SlintButton };

export component GlobalSettings inherits Window {
    property<string> campus_search_query: "";
    property<vector<string>> search_results: [];
    property<float> selected_anchor_lng: 0.0;  // ← NEW
    property<float> selected_anchor_lat: 0.0;  // ← NEW
    
    callback confirm_campus();
    
    on-confirm-cupus clicked:
        if (selected_anchor_lng != 0 && selected_anchor_lat != 0) {
            api.select_campus(campus_search_query, selected_poi_id, 
                             selected_anchor_lng, selected_anchor_lat);
            emit(confirm-campus());
        }
    
    // Map area with draggable marker
    canvas map_area {
        height: 300px;
        on-clicked: {
            let pos = get_mouse_position(event);
            let geo = map_pos_to_geo(pos.x, pos.y);
            selected_anchor_lng = geo.lng;
            selected_anchor_lat = geo.lat;
        }
    }
}
```

### Step 7: ViewModel Binding (`apps/desktop/src/injector.rs`)
```rust
// Inject campus search handler

fn bind_campus_search(vm: &CampusSearchViewModel, injector: &ViewModelInjector) {
    vm.on_confirm.register(move |query, poi_id, lng, lat| {
        let settings = injector.settings.clone();
        settings.borrow().select_campus(query, poi_id, lng, lat).unwrap_or_else(|e| {
            // Show error modal
            notification_center::show_error("Failed to save campus", Some(e));
        });
    });
}
```

### Step 8: Internationalization
```json
// core/localization/resources/zh-CN.json

{
  "f1.campus.search.placeholder": "搜索校区名称",
  "f1.campus.confirm.button": "确认选择",
  "f1.campus.map.drag_hint": "拖动地图以调整中心点",
  "f1.errors.invalid_campus": "请选择一个有效的校区"
}
```

---

## 🧪 测试策略

### Unit Tests

```rust
// tests/unit/campus_schema_migration.rs

#[test]
fn test_add_anchor_columns_schema() {
    let db = Database::open_in_memory().expect("内存库");
    migrations::run_migration(&db, 3).expect("Migration v3 success");
    
    // Check columns exist
    let table_info: Vec<_> = db.prepare("PRAGMA table_info(campuses)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .flatten()
        .collect();
    
    assert!(table_info.contains(&"anchor_lng".to_string()));
    assert!(table_info.contains(&"anchor_lat".to_string()));
}

#[test]
fn test_campus_repository_create_with_anchor() {
    let db = Database::open_in_memory().expect("内存库");
    let repo = CampusRepository::new(db);
    
    let campus_id = repo.create_with_anchor(
        "北京大学", "239494", 
        116.308, 39.995  // 清华北大附近
    ).expect("Insert campus");
    
    let campus = repo.find_by_id(campus_id).expect("Get campus");
    assert_eq!(campus.anchor_lng, 116.308);
    assert_eq!(campus.anchor_lat, 39.995);
}
```

### Integration Test

```rust
// tests/integration/t05_anchor_persistence.rs

#[test]
fn test_full_flow_anchor_saved_and_reloaded() {
    // 1. Setup in-memory DB with migration v3
    let tmp_dir = TempDir::new().unwrap();
    let db_path = tmp_dir.path().join("test.db");
    let _ = Database::open(&db_path);
    
    // 2. User selects campus via F1 UI (mocked)
    let mut settings = SettingsManager::new(Database::open(&db_path).unwrap());
    settings.select_campus("清华大学", "239494", 116.308, 39.995).unwrap();
    
    // 3. Simulate app restart
    let mut new_settings = SettingsManager::new(Database::open(&db_path).unwrap());
    
    // 4. Verify anchor persisted
    let last_campus = new_settings.landing_campus().unwrap().expect("Has campus");
    
    // Should have correct anchor, not default Beijing
    assert_ne!(last_campus.anchor_lng, 116.397);  // ≠ default
    assert_eq!(last_campus.anchor_lng, 116.308);
    assert_eq!(last_campus.anchor_lat, 39.995);
}
```

---

## 📊 Acceptance Criteria Checklist

- [ ] Schema migration script exists and runs successfully
- [ ] Campus model has `anchor_lng` and `anchor_lat` fields
- [ ] Repository method `create_with_anchor()` implemented
- [ ] Gaode search API returns center coordinates
- [ ] F1 UI shows search list + interactive map
- [ ] Confirmation button stores anchor to DB
- [ ] App reload uses saved anchor instead of default
- [ ] Unit tests cover all new functions (> 90% coverage)
- [ ] Integration test validates end-to-end persistence
- [ ] zh-CN.json keys added for all new strings
- [ ] `.slint` files validated for zero hardcoded text/colors
- [ ] Workspace build passes (`cargo build --workspace`)
- [ ] All CI gates green (fmt/clippy/test/machete/deny)
- [ ] Handoff document written for next session

---

## 🚀 Expected Outcome

完成后：
- ✅ 用户首次配置时选择的校区会被记住
- ✅ 下次启动高德地图自动聚焦到该校区
- ✅ 边界绘制时的 OSM 返回结果在正确区域
- ✅ GCJ-02 转换有合理的锚点基准
- ✅ 消除"每次都回到北京"的体验断层

---

## 📝 Related Docs

- [`docs/adr/0004`](../../docs/adr/0004-app-level-global-settings.md) — 应用级全局设置
- [`docs/adr/0008`](../../docs/adr/0008-campus-selected-via-gaode-search.md) — 高德搜索选定校区
- [`handoff-2026-07-28-t25-complete.md`](../../handoff-2026-07-28-t25-complete.md) — T25 交接文档中已知限制 #1
- [`sqlite/schemas/v1.sql`](../../../sqlite/schemas/v1.sql) — 数据库设计草案

---

## 💡 Technical Notes

### 关于 GCJ-02 坐标系
- 高德地图使用 **GCJ-02** (国测局坐标系，火星坐标系)
- OSM/WGS84 是国际标准 GPS 坐标
- B5 foundation-mode 负责转换：**GCJ-02 → WGS-84**
- anchor_lng/anchor_lat 应该存为 **GCJ-02**（高德内部格式），避免偏差

### 关于默认值风险
- 默认北京坐标会导致所有新用户初始偏差
- **解决方案**: 在第一次搜索前不加载地图，而是提示用户先搜索
- 或者：使用 IP 定位服务获取大致位置（可选增强）

---

## 🙋‍♂️ Dependencies & Blockers

**阻塞**: Deny 红项修复（TXX-DENY-FIX）必须先完成  
**依赖**: 
- ADR-0004 (全局设置持久化) ✅ 已决定
- ADR-0008 (高德搜索选校区) ✅ 已决定
- B2 data-persistence (campus repository 扩展)  
- B3 gaode-client (POI 搜索增强)

**预估工时**: 1.5 - 2 天（含验证时间）

---

**开始条件**: Deny 红项豁免已通过 CODEOWNERS 审批  
**预计提交**: Single PR after TXX-DENY-FIX merge  
**风险评估**: 中低风险（核心流程改动大但路径清晰）
