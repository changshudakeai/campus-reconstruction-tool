# T19B-3 — 老用户二次启动逻辑 + 校区选择页

**What to build**: 老用户打开软件时的"着陆判定"——如果之前用过就自动跳到上次使用的校区方案列表页；如果是新校区或上次校区被删了就显示校区选择页（高德搜索）。

- **窗口契约**: 缝 1（Shell ↔ F1），读 `last_campus_id` 并翻译成败局路由决策
- **业务规则**: 老用户直达上次校区 (ADR-0006)；校区已被删除则安全退回选择页
- **UI 决策法源**: ADR-0027（五步步骤条向导骨架 / 跳转规则 / 右上角四入口，含“切换校区”常驻入口）+ ADR-0028（教程三泡清单）；与本单描述冲突时以 ADR 为准

**Blocked by**: T19B-2 (首次向导已完成)

**Status**: completed（2026-07-27 commit b72e2a1：校区选择页与五屏路由接线）

## 🎯 验收标准

### 核心交付物
- [ ] `runtime.rs` 中的 `landing_decision()` 函数完善以下逻辑：
  ```rust
  match settings.is_first_run() {
      Ok(false) => match settings.landing_campus() {
          Ok(Some(campus)) if campus_exists(&campus.id) 
              → LandingDecision::LastUsedCampus { name }
          _ → LandingDecision::CampusSelect,
      },
      Ok(true) → LandingDecision::FirstRunSetup,
  }
  ```
- [ ] 校区选择页 UI (`campus_select.slint`)：
  - 高德地图搜索框（调用 B3 GaodeClient）
  - 搜索结果列表（POI 筛选）
  - "确认选用"按钮
- [ ] 选中校区后保存到 `app_settings.last_campus_id` 并跳转方案列表页

### 文案与国际化
- [ ] zh-CN.json 新增：
  ```json
  {
    "shell.status_campus_select": "欢迎回来！请选择或新建校区",
    "shell.campus_deleted_notice": "上次使用的校区已被删除，请重新选择校区",
    "campus_search.placeholder": "搜索学校名称",
    "campus_select.confirm_button": "确认选用"
  }
  ```

### 架构断言（CI 门禁必过）
- [ ] `cargo xtask arch` 通过：shell 不直接依赖 B3（高德客户端应经 F3 功能模块中转）
  - ⚠️ **注意**：这里可能是 ADR-0017 的例外情况，需要验证文档
- [ ] `cargo deny check bans` 无违规跨层调用

### 数据一致性
- [ ] 单元测试：`test_landing_campus_returns_none_if_deleted()` 断言校区被删后返回 None
- [ ] 数据库事务：保存 last_campus_id 必须是原子操作

### 用户体验
- [ ] 老用户第二次打开软件时顶部显示上次校区的名字
- [ ] 顶部显示"切换校区"按钮，点击后返回校区选择页
- [ ] 如果数据库里有多个校区能正常看到列表

## 📋 实施提示

### 技术实现细节
1. **高德搜索集成**：
   - 如果 Shell 不允许依赖 B3，则在 F3 (`ProjectManager`) 里加一个 `search_campuses(query: &str)` 方法
   - Shell 调用 `f3.search_campuses(...)` 得到结果列表并展示

2. **校区 ID 存储格式**：
   ```json
   {
     "key": "last_campus_id",
     "value": "campus_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
   }
   ```

3. **导航决策树**：
   ```
   ┌─ 首次运行？─ Yes → F1 设置向导 (T19B-2)
   │
   No
   ├─ last_campus_id 存在且有效？─ Yes → 直达该校区方案列表
   │                               (T19B-4 完成后衔接此处)
   │
   └─ No (未设置 / 校区被删) → 校区选择页
   ```

### 避免的错误
- ❌ 不要在 Shell 里写 SQL 查询校区是否存在
- ✅ Shell 应该委托 F3 做这个判断：`f3.is_campus_valid(&campus_id)`

## 💡 特别提示
这是贯穿弹剧本的关键枢纽——第一次用的人走向导→选校区→建方案；老用户直接进校区→看方案列表。**T19B-4 必须假设可以从这个页面的某处跳转到它**。

✅ **收工自检清单**:
- [ ] `cargo check` 全 workspace 无报错
- [ ] 手动测试：第一次启动→向导→选校区；第二次启动→直达该校區
- [ ] 破坏性测试：手动改数据库删除校区 → 启动 → 能看到选择页而不是崩溃
- [ ] `cargo clippy --workspace -- -D warnings` 零警告
- [ ] `cargo fmt --all --check` 格式化通过

---

## 负责人验收点（一句话）

第一次启动能看到向导选语言和 MC 版本，第二次启动能看到顶部显示校区名并且能直接进方案列表页；如果我把数据库里的校区删了再启动能看到选择页而不是崩溃。
