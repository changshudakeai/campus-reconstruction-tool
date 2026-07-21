# Campus Reconstruction Tool V1.1.0

面向 Minecraft 校园复刻的 Windows 11 x64 原生工作台。V1.1.0 需要联网完成高德校区搜索以及新增/刷新 Foundation 采集；项目、审核、生成和导出保留在本机。

## V1.1.0 功能

- 校区优先的 schema 2 项目库、自动边界候选、五类 Foundation 证据审核、生成与导出。
- 高德只负责校区目标确认；Foundation 证据仅通过认证、版本化的受控服务取得。
- V1.1 不提供五类空白画布绘制、截图恢复、公共 Overpass 回退或直接提供方采集。
- V1.0.1 解码器仅用于显式、事务化迁移；所有生产写入均使用 schema 2。
- 候选接受/拒绝是主流程，不再放在补缺或高级折叠区；没有右侧地基清单卡片。
- 精细建筑使用 Arnis 2.9.0 的 19 类固定外观规则。实测轮廓、高度和楼层不被模板修改；模板只改变方块、窗户、墙面层次、屋顶线和装饰。
- 原生 wgpu 方块预览；Foundation 与精细建筑均可导出 gzip 压缩的 Sponge V3 `.schem`。
- 原子自动保存、恢复副本、撤销/重做、Ctrl+S/Ctrl+Z/Ctrl+Y。
- 中英文界面同步覆盖 Slint 主程序、高德工具和原生预览；偏好不写入便携工程。
- 可将受支持的 V1.0.1 本地或便携项目迁移为 schema 2；旧格式不会再被写入。

照片匹配、中国高校风格模型训练与模板在线推荐属于后续版本，不在 V1.1.0 中虚构实现。

## 本地与服务端边界

必须留在本地：

- 项目、审核决定、生成模型和 `.schem`；
- Arnis 规则与生成器；
- Slint UI、wgpu 预览；
- 高德密钥（Windows 凭据管理器）。

适合放在云端：

- 受控 Foundation 数据采集、版本化 Dataset Bundle 与共享缓存；
- 模板/应用版本清单；
- 可选的共享校园标注；
- 未来独立网页伴侣。

云端不可用时，已有项目仍能打开、编辑、生成和导出。详见 [deployment-boundary.md](docs/deployment-boundary.md)。

## 开发

要求 Windows 11 x64 和 Rust stable：

```powershell
npm run dev
npm test
npm run build
```

小范围修改优先使用局部反馈命令，避免每次都编译和测试完整工作区：

```powershell
npm run check:ui
npm run test:ui
npm run test:state
npm run test:services
```

`npm run dev` 会先增量检查地图与预览工具，再在当前终端运行主程序。应用内“日志”按钮可打开
`%LOCALAPPDATA%\CampusReconstructionTool\logs`；普通操作错误、工具进程错误和崩溃都会写入带事件编号的 JSONL 会话日志。

高德密钥在应用内“地图设置”保存。Foundation 采集服务通过 `CAMPUS_ACQUISITION_SERVICE_URL` 配置，并且必须提供兼容、版本化的 `/v1` 合同；服务不可用时暂停新采集，不切换数据提供方。

## 打包

```powershell
npm run candidate:v1.1 -- -CleanWindowsImageId "<clean-image-id>" -CleanWindowsImageManifest "<image-manifest.json>"
npm run size:release
```

候选目录输出到 `artifacts/candidates/<candidate-id>/`，其中包含提交、环境、命令、测试计数、日志、退出状态、二进制与安装包 SHA-256。打包只消费已经验证的三个 release 可执行文件，不会重新构建；安装包大小仅记录，不设 50 MB 门槛。

## 代码布局

- `native/apps/campus-native`：Slint 主程序与唯一应用状态入口。
- `native/apps/campus-map`：独立 WebView2 高德工具。
- `native/apps/campus-preview`：独立 wgpu 原生预览。
- `native/crates/campus-state`：项目、自动保存、兼容导入、撤销/重做。
- `native/crates/campus-services`：受控 `/v1` 采集客户端、版本化 Dataset Bundle 与覆盖率验证。
- `native/crates/campus-export`：Sponge V3 `.schem`。
- `native/crates/arnis-core`：固定上游版本的 Arnis 建筑规则移植。
- `services/`：受控 Foundation 采集服务与开发工具。
