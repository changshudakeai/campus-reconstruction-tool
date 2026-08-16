# T05 - A: Deny 红项处理（wry 传递依赖 MPL-2.0 豁免）


**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

**工单编号**: TXX-DENY-FIX (或沿用 T05-A)  
**优先级**: ⭐⭐⭐⭐⭐ (阻塞 CI 全绿)  
**关联**: T21 (地图集成), T24/T25 (边界/朝向), handoff-2026-07-28-t25-complete.md  
**状态**: `completed`（2026-07-28 commits 7308b9b / d461510 / b079b5c：MPL-2.0 豁免与文档链修复）

---

## 🎯 问题描述

### 现状
```bash
cargo deny check licenses --workspace
```
输出 **红灯**：
```
license_id = "MPL-2.0" of crate "gtk-rs@*"
reason = "unlisted license"
```

### 根因分析
- wry 0.55 (高德地图嵌入核心库) 引入 gtk-rs 作为 Linux 平台依赖
- gtk-rs 使用 **MPL-2.0** (Mozilla Public License 2.0)
- deny.toml 的许可证白名单中缺少此条款
- 这是 WIP(Work In Progress) 传递依赖，不是直接引入

### 影响范围
- CI dependencies job 始终显示红灯
- 无法标记为"完整绿灯"
- 符合手递手建议："守门人决策后豁免"

---

## ✅ 验收标准

1. **deny.toml 更新**:
   - 在 `[licenses].allow` 区域添加 `"MPL-2.0"`
   - 附注说明来源：`# wry → gtk-rs (Linux 托盘支持)`

2. **CI 验证**:
   ```bash
   cargo deny check licenses --workspace
   # 返回 0 (全绿)
   
   cargo deny check bans sources advisories --workspace
   # 全绿（无新增依赖故不影响）
   ```

3. **CODEOWNERS 审查**:
   - PR 通过 CODEOWNERS 路由至原维护者（T21 提交者）+ 产品负责人
   - 获得至少 1 个 LGTM

4. **handoff 记录**:
   - 在手递手文档末尾添加变更记录
   - 注明豁免理由、日期、执行者

---

## 🔧 实施步骤

### Step 1: 修改 deny.toml
```toml
[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    # ... existing entries ...
    "MPL-2.0",  # wry → gtk-rs (Linux 托盘支持)
]
```

### Step 2: 本地验证
```bash
cd New-branch-v2
cargo deny check licenses --workspace
cargo deny check bans --workspace
cargo deny check advisories --workspace
cargo deny check sources --workspace
```

预期输出：
```
✅ All checks passed!
No unlisted licenses found.
No banned crates found.
No security advisories flagged.
All dependencies from known registries.
```

### Step 3: 测试现有构建
```bash
cargo build --workspace --release
# 确保没有编译错误（MPL-2.0 许可不影响编译）


```

### Step 4: 提交代码
```bash
git add deny.toml
git commit -m "[TXX-DENY-FIX] Add MPL-2.0 exception for wry→gtk-rs (Linux tray)"
git push
gh pr create --title "Fix: MPL-2.0 license exemption for wry gtk-rs dependency" \
              --body "Closes #TXX-DENY-FIX
             
## Summary
Added MPL-2.0 to license whitelist with explanation comment.
This is a transitive dependency of wry (map WebView) for Linux tray support.

## Changes
- docs/agents/deny.toml: added MPL-2.0 allowance

## Verification
- cargo deny check licenses → ✅ green
- cargo build → ✅ success
"
```

---

## 📝 技术背景说明

### 为什么允许 MPL-2.0？

1. **许可性质**: MPL-2.0 是弱著佐权许可证（weak copyleft）
   - 仅对修改后的源代码要求开源
   - 作为静态链接的传递依赖，不触发传染性
   - 与 Apache-2.0/MIT 兼容

2. **实际风险低**:
   - 仅限 Linux 平台的托盘图标功能
   - Windows/macOS 平台不使用 gtk-rs
   - 本项目核心桌面体验不受影响

3. **上游策略一致**:
   - Slint 项目同样豁免 MPL-2.0（见 slint 官方 Cargo.toml）
   - bevy/rustc 等主流项目均有类似豁免

### 为什么不直接修改 wry?

- wry 是第三方库，我们只负责调用 API
- Linux 支持由 wry 团队维护
- 我们的策略是：**接受上游依赖的许可复杂性**，而非自行裁剪

---

## 🚫 不适用场景警告

以下情况**不应**直接加入 MPL-2.0：

❌ **如果你的项目直接引入了 GPL-3.0 依赖**  
→ GPL 的强传染性可能污染你的整个二进制文件

❌ **如果依赖是为了实现跨平台 UI 框架（如 Qt）**  
→ Qt 有双重许可（LGPL/GPL），需要更详细的法律评估

✅ **本项目的适用性**:
- 单一用途桌面应用（非通用 UI 框架）
- LGPL-2.1/MPL-2.0 来自被动传递依赖（wry 内部）
- 不涉及对 gtk-rs 的直接修改或分发

---

## 📋 相关文档

- [`docs/adr/0003`](../../docs/adr/0003-rust-slint-stack-reuse-core.md) — 复用 v1.x 导出引擎/Arnis 规则/地图逻辑
- [`docs/research/gaode-map-integration-options.md`](../../docs/research/gaode-map-integration-options.md) — T21 地图集成方案调研
- [`handoff-2026-07-28-t25-complete.md`](../../handoff-2026-07-28-t25-complete.md) — T25 交接文档

---

## 🙋‍♂️ 待决事项

1. **产品负责人确认**:
   - 是否同意将 MPL-2.0 加入白名单？
   - 是否需要更多法律评估？

2. **守门人审批**:
   - PR 需 CODEOWNERS 审批
   - 至少 1 名核心贡献者 LGTM

3. **长期跟踪**:
   - 关注 wry 升级计划（是否会移除 gtk-rs？）
   - 跟踪 Rust ecosystem 中 GPL/MPL 依赖的趋势

---

## 🎁 预期成果

完成后：
- ✅ CI dependencies job 从红色变为绿色
- ✅ project 标注为"CI 全绿除必要的 MPL-2.0 豁免外"
- ✅ 后续引入新依赖时已有明确的许可证管理模板
- ✅ 减少一次重复性的"为什么还有红灯"追问

---

**开始条件**: 产品负责人同意上述技术方案  
**预计工时**: < 30 分钟（主要是验证时间）  
**风险评估**: 低风险（已在其他项目验证过）
