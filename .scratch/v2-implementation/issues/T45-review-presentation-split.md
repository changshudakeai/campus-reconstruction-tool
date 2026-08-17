# T45 — 拆出评审建议呈现翻译

**Status:** ready-for-agent

**What to build:** 从 `production/review.rs` 拆出轻量建议筛选、一键应用和撤销上一批
的呈现翻译，建立模块内部私有 seam，删除文件长度期限豁免；不得改变评审三态、
地图标注或用户文案。

**Blocked by:** T41.

## 验收与验证

- [ ] 两侧职责可分别用一句话说明，外部 ReviewProductionAdapter 接口不扩大。
- [ ] desktop-shell 相关 review 定向测试 + crate Clippy + fmt +
  `.\scripts\cargo-managed.ps1 -- xtask tidy`。
- [ ] 同 crate 私有搬分不触发 timings；PR 收口完整门禁一次。
