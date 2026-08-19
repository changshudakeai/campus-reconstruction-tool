# T52 第五步 3D 方块预览——真机走查证据（2026-08-19）

环境：Windows、`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`、开发版二进制
`target\debug\campus-tool-dev.exe`（本分支构建），真实校区
「华东师范大学普陀校区 / 新方案 1」（Overpass 实时采集 1668 个可评审候选）。

## 操作步骤与观察

1. 进入第五步：抽屉出现显眼的「生成 3D 预览」按钮；预览状态为空，预览页
   不加载任何生成数据——`进入第五步不自动生成`。
   - 证据：`step5-restored-no-autogen.png`（重启恢复第五步后同样不自动生成）。
2. 点击「生成 3D 预览」：后台生成，抽屉显示
   「3D 预览已生成：918232 个方块，与导出内容一致。」
   - 证据：`step5-preview-generated.png`。
3. 旋转/缩放/复位：左键拖拽旋转、滚轮缩放、抽屉「复位视角/放大/缩小」按钮。
   - 证据：`step5-preview-rotated.png`、`step5-preview-zoomed.png`、
     `step5-preview-reset.png`。
4. 帧率：预览页每 2 秒回传 `preview_stats`，稳态 160–165 fps（目标 30–60）；
   918,232 方块经同色面贪婪合并后仅 6 个可见面四边形。
   - 日志摘录（`t52-walkthrough.log`）：
     `preview_loaded {"blocks":918232,"quads":6,"simplified":false}`
     `preview_stats {"fps":165,"blocks":918232,"quads":6}`
5. 导出衔接（基础导出）：导出完成显示「导出完成」与
   「1064×1×863」尺寸，同区域继续显示 3D 预览，预览状态保留。
   - 证据：`step5-after-export.png`；`59d53f43-…schem` 基础版 1162 字节。
6. 超大方案自动简化并提示：增强预览（基础场地 + 保留候选 3 项）包围盒达
   1285 万格，弹出「3D 预览已简化——方案较大，预览已自动简化渲染；方块数据
   与导出内容一致。」
7. 预览失败不阻塞导出：在真实 WebView2 快速重建时出现一次性创建失败
   （`HRESULT 0x80070057`），界面给出「3D 预览生成失败。可以继续导出，导出
   流程不受影响。」错误弹窗；导出随后照常完成。
8. 增强导出块级一致：预览「974121 个方块」= 导出的增强 `.schem` 非空气方块
   数 974,121，逐块解码（Python NBT）：

   | 方块 | 数量 | 类别 |
   |---|---|---|
   | minecraft:stone_bricks | 918,231 | 平整场地 |
   | minecraft:bricks | 4,750 | 建筑墙体 |
   | minecraft:oak_planks | 23,840 | 建筑地板 |
   | minecraft:dark_oak_slab | 25,100 | 建筑屋顶 |
   | minecraft:glass_pane | 2,176 | 建筑窗户 |
   | minecraft:dark_oak_door | 4 | 建筑入口 |
   | minecraft:oak_log | 3 | 植被树干 |
   | minecraft:oak_leaves | 17 | 植被树叶 |

## 已知环境性说明

- 走查期间与 T51 并行会话共享桌面与 WebView2 运行时，对方批量回收同名进程
  并多次移动窗口，导致本走查的窗口被移动/关闭；本证据已排除对方实例，只
  记录本分支二进制（改名为 `t52-preview-walk.exe`）的窗口。
- 快速隐藏→重建预览页时 WebView2 偶发 `0x80070057` 创建失败；该失败被现有
  地图会话失败路径如实呈现，重新进入第五步即可重试，不阻塞导出。
