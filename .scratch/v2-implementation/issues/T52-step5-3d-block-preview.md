# T52 — 第五步 3D 校园方块预览

**Status:** completed（2026-08-20 产品负责人真机验收通过；PR #25 draft，待合并）

> 2026-08-20 验收迭代记录：第一版（砖纹矩形底座）验收不通过；按产品负责人
> 反馈完成三轮修复后复验通过：① 底座按真实边界多边形裁剪（不再矩形）；
> ② 底座表层由 stone_bricks 改为 grass_block；③ 图集 mipmap padding +
> 线性过滤 + 提高光照，远景不再噪点、方块纹理清晰可辨。最终实现同时
> 修复了 textureMap 注入拼写、UV/几何坐标分离、外向绕序、法线计算、
> 分块 key 单射、保留页代际、barrier 家族生成等缺陷（见 PR #25 提交历史）。
> “提升导出 3D 精度（更高清纹理/更精细逐块渲染）”已按产品负责人要求计入
> 后续升级方案（主线计划“明确不做的剩余产品项”）。

**What to build:** 把五步工作流第五步（导出）中原先的高德地图显示替换为
“整校园 3D 方块预览”（参考 mcblock.top 的方块效果）：建筑、道路、水面、植被
以真实 Minecraft 方块呈现，可旋转/缩放/复位；预览数据与最终导出的 `.schem`
完全一致。

## 已确认的产品决策（2026-08-18 访谈）

1. **预览内容**：整方案（校园级）3D 方块视图，不是单建筑逐层教学；建筑/道路/
   水面/植被都用真实方块渲染。
2. **数据来源**：必须是 B18 生成引擎的真实 `BlockModel` 生成结果，与最终导出的
   `.schem` 一致——预览里看到的就是导出的内容；不另造一套预览模型。
3. **生成时机（防误点）**：进入第五步不自动生成；只有用户点击显眼的
   “生成 3D 预览”按钮后才开始生成。
4. **交互**：支持旋转、缩放、复位；典型校园场景目标 30–60fps 流畅旋转。
5. **导出衔接**：导出完成后同区域继续显示 3D 预览，并叠加导出结果状态。
6. **失败处理**：预览生成失败给出明确错误提示，不阻塞导出流程。
7. **影响范围**：只替换第五步的地图显示；第一/二/三/四步的地图行为完全不变。

## 技术方向（实施者定，调研已给出推荐）

- 调研结论：mcblock.top 是 Next.js（React）网页应用，预览大概率是 DOM/CSS 等距
  方块渲染（抽查 chunk 未见 WebGL 特征），适合单建筑逐层教学，不适合校园级
  数万~数十万方块，且无公开源码可复用。
- 推荐方向：复用现有 WebView（wry/WebView2）通道内嵌 Three.js；Rust 侧把 B18
  `BlockModel` 序列化为渲染数据（方块 ID + 坐标），前端做隐藏面剔除、同类方块
  合并/实例化、屏幕外剔除。
- 可复用开源库（均 MIT，商用可用）：craftmatic（`.schem` 原生支持，与导出格式
  直接相关）、deepslate / @mattzh72/lodestone（MC 渲染成熟）。实施时优先评估
  craftmatic 与 lodestone；不引入完整 Rust 游戏引擎。
- 超大方案自动简化并提示；预览失败不影响导出。

## 验收与验证

- [x] 进入第五步不自动生成；点击“生成 3D 预览”后才开始，按钮位置明显。
- [x] 预览与 B18 生成结果一致：抽查建筑/道路/水面/植被方块与导出 `.schem`
  对得上。
- [x] 旋转/缩放/复位可用；典型校园真机旋转流畅（目标 30–60fps）。
- [x] 预览失败有明确错误提示，且导出流程不受影响（“简化提示”语义已随
  分块渲染重构删除，不再需要超大方案降级）。
- [x] 导出完成后同区域继续显示预览 + 导出结果状态。
- [x] 第一/二/三/四步地图行为不变。
- [x] 既有契约测试更新/新增：第五步“生成 3D 预览”按钮、候选卡片定位、
  导出衔接、无候选定位安全与预览定位命令探针。
- [x] 定向门禁（T41 分级）：涉及 desktop-shell 呈现层与 WebView 通道，先跑
  desktop-shell 定向测试 + Clippy + fmt + tidy；若新增前端资源/打包规则，按对应
  crate 门禁处理。

## 最终收口证据（2026-08-20）

- PR：https://github.com/changshudakeai/campus-reconstruction-tool/pull/25
- 真机日志：`%TEMP%\t52-r11-acceptance.log`（验收轮次日志）
- 真实校区负载分析：底座多边形裁剪后 659,928 方块（行宽 6–716 不等），
  palette 含 `grass_block`；Node 全链路 168 块 / 56,196 quads / 0 空块。
- 门禁（最终 HEAD）：`test -p foundation-mode -p generation-engine
  -p export-console -p export-flow`、`test -p desktop-shell -- --skip
  fetch_stability`、`clippy ... --all-targets -- -D warnings`、`fmt --all
  --check`、`xtask tidy` 全部通过。
- 未新增 Cargo 依赖 / npm 构建步骤，machete / deny / timings / workspace
  tests 按触发条件未运行（如实记录）。

## 升级门禁触发项

- 涉及 desktop-shell（第五步呈现层 + WebView 通道）；若新增 npm 前端资源、
  构建步骤或调整依赖白名单，升级为完整门禁（machete/deny/workspace 全量/
  timings）。
