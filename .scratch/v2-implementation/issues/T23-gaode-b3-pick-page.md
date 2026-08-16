# T23 — B3 地图页升级 JS API 2.0 + 取点协议

> 法源声明：与工单描述冲突时以 ADR 为准。重点条款：ADR-0017（B3 职责含
> 坐标拾取、边界绘制）、ADR-0005（文本外置）、ADR-0021（高德接口拒绝 =
> 要紧错误，弹窗）。
> 规格来源：`.scratch/v2-implementation/PRD-gaode-map-integration.md`。
> 技术依据：`docs/research/gaode-map-integration-options.md`（路线 A）；
> `.scratch/map-demo/`（点击回传协议已验证）。

**What to build（负责人视角）：**
B3 高德模块升级后能使用新申请的高德 key（新 key 必须搭配安全密钥，旧版
模板不支持）；模块新提供一种"取点地图页"——在地图上点一下，这一点的
经纬度就传回程序；地图加载失败或 key 被高德拒绝时，程序能察觉并弹窗告知。
本单交付的是**模块能力**：点击取点的真实手感归 T24 验收，本单的端到端
验证走设置页"测试连通性"（T22 已建）+ 单测锁定。

**Blocked by：** T22（密钥配置——key 从设置页来）。

**Status：** completed（2026-07-28 commit ec101f3：取点页与 IPC 协议）

## 验收标准

- [ ] 地图页模板升级 JS API 2.0：securityJsCode 在脚本加载**之前**注入；
  key 防注入校验沿用既有规则（纯字母数字，其余拒绝）
- [ ] 新增取点地图页：点击地图 → `window.ipc.postMessage` 回传 `经度,纬度`
  （demo 已验证协议）；与既有校区搜索页并存
- [ ] 桥协议收敛为 `window.ipc.postMessage` 唯一通道；既有
  `window.mcrebuildBridge` 如需保留，经初始化脚本做别名，不另开通道
- [ ] 地图加载失败 / key 被拒的检测（脚本错误监听 + 超时心跳）→ 结构化
  错误回传壳层，壳按 ADR-0021 走 B7 弹窗
- [ ] 单测锁定：v2.0 URL、securityJsCode 注入位置、防注入拒绝、取点页含
  ipc 回传脚本、错误检测脚本存在；协议解析单测（坐标 / 错误 / 畸形载荷）
- [ ] 设置页"测试连通性"走 v2.0 + 安全密钥链路：真 key 探活成功、错 key
  弹窗（与 T22 联验）
- [ ] `centerOn(锚点)` 能力沿用（ADR-0008：画边界从锚点开始）

## 通用收工标准

- [ ] 全部门禁绿灯：cargo fmt / clippy / test / machete / deny / xtask ci +
  GitHub Actions run success
- [ ] 文案键独立 commit（zh-CN.json 高冲突铁律）
- [ ] 验收逐条对照代码证据打勾
- [ ] 占位项诚实声明写入完成汇报
