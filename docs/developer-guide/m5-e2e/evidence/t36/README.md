# T36 真机走查证据（2026-08-09，100% DPI）

构建：`fix/t36-orientation-map` @ 9bcf816，release 二进制在隔离沙箱
`D:\MCRebuild_Renovation\.t36-walkthrough\`（未覆盖 `MCRebuildV2\dev`
安装目录——并发会话正在使用该目录）。

## 已验证（map.log 逐条证据）

1. 打开方案 → 边界页 WebView 创建 → `notify_status(available=true)`；
2. 页面 `map_ready` IPC → Rust 侧 OSM 自动获取（Nominatim → Overpass
   way 288249651，上交闵行 39 点环）→ 自动绘制；
3. 抽屉"确认边界" → `confirm_boundary` IPC（931 字节）→ B5 校验通过 →
   "边界已确认，可点'重置'重新绘制"；
4. 步骤②切换：`hide()` 排定下一拍销毁 → **事件循环下一拍实际 drop 旧
   WebView（IPC 回调返回后）** → 之后才 `开始异步创建 WebView
   （page=Orientation, generation=5）` → 创建成功 + `notify_status(true)`
   ——T36 的 hide→show 串行化（retiring 队列清空后才新建）得到真机验证；
5. 无效边界确认（90 点 OSM 环，T33 未合入 main 的已知自相交）→ 明确
   错误弹窗"边界自相交：第 0 条边与第 86 条边相交"，程序不退出，弹窗
   关闭后地图按当前步骤重建。

## 仍未通过（P1 根因，真机复现）

- 步骤②朝向页 WebView 已创建、`initOrientationMode` 已运行（状态面板
  "已加载已确认边界作半透明参照"），但**真实/合成鼠标点击均无法到达
  页面 JS**（`mouse_event`、`SendInput`、`PostMessage WM_LBUTTONDOWN/UP`
  四种注入均无 `orientation_points` IPC 产出）。这正是工单标题
  "设定朝向步骤点击地图无反应"的现场复现。
- 结论：生命周期串行化修复了"旧 WebView 未销毁即新建"的时序窗口，但
  输入不达页面是独立问题（疑似 WebView2 151.0.4129.72 子窗口输入/
  焦点）。ticket 备选方案"单 WebView 复用、按页换内容"尚未实施验证；
  s1_26 用 `evaluate_script` 驱动真实朝向页锁死 JS 链路（通过），但
  不能覆盖真实鼠标输入路径。
- 附带发现：边界页抽屉"重置"走 `clearManualDrawingFromDrawer`，不重新
  注册人工圈画 click 处理器，重置后地图点击同样无反应（独立的呈现层
  缺陷，建议单独立项）。
- 125% DPI 走查需按 T35 记录先刷新 Windows 会话（登出/登入），本机
  会话被并发任务占用，未执行。

## 文件

- `map-t36-walkthrough.log.txt`：MCREBUILD_LOG_FILE 采集的 map_webview
  生命周期/IPC/销毁日志（本次走查全程；*.log 被 gitignore，故以 .txt 留存）。
- `01-start.png`：首个实例启动（方案列表）截图。
- `02-boundary-osm-confirmed.png`：上交闵行 OSM 边界自动选取并确认后截图。
