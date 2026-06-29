# Campus Reconstruction Tool V1

面向 Minecraft 校园复刻的 Windows 原生工作台。V1 主程序使用 Slint/Rust，不是网页外壳；只有必须使用高德 Web JS API 的地图工具运行在独立 WebView2 子进程中，方块模型预览是独立 Rust/wgpu 子进程。

## V1 功能

- 九步地基流程：校区、边界、朝向、建筑、道路、水域、植被、体育、导出。
- 高德 3D 半自动取景：隐藏地图标签，用户自行调整缩放、俯仰和平移，再按“截取并识别当前视野”。
- 地图视角、取景范围和手绘边界随项目自动保存，下次打开继续上次状态。
- OSM/Overpass 校区地物识别；配置云端 Overture 服务后合并更完整的建筑轮廓。
- 候选接受/拒绝是主流程，不再放在补缺或高级折叠区；没有右侧地基清单卡片。
- 精细建筑使用 Arnis 2.9.0 的 19 类固定外观规则。实测轮廓、高度和楼层不被模板修改；模板只改变方块、窗户、墙面层次、屋顶线和装饰。
- 原生 wgpu 方块预览；Foundation 与精细建筑均可导出 gzip 压缩的 Sponge V3 `.schem`。
- 原子自动保存、恢复副本、撤销/重做、Ctrl+S/Ctrl+Z/Ctrl+Y。
- 可导入旧 React/Tauri V1 便携项目 JSON。

照片匹配、中国高校风格模型训练与模板在线推荐属于 V2，不在 V1 中虚构实现。

## 本地与服务端边界

必须留在本地：

- 项目、审核决定、生成模型和 `.schem`；
- Arnis 规则与生成器；
- Slint UI、wgpu 预览；
- 高德密钥（Windows 凭据管理器）。

适合放在云端：

- Overture GeoParquet 查询与共享缓存；
- 模板/应用版本清单；
- 可选的共享校园标注；
- 未来独立网页伴侣。

云端不可用时，已有项目仍能打开、编辑、生成和导出。详见 [deployment-boundary.md](docs/deployment-boundary.md)。

## 开发

要求 Windows 10/11 x64 和 Rust stable：

```powershell
npm run dev
npm test
npm run build
```

网页兼容实现不再是桌面入口，仅供未来云端伴侣迁移参考：

```powershell
npm run web:dev
npm run web:build
```

高德密钥在应用内“地图设置”保存。可选云端 Overture 地址通过 `CAMPUS_DATA_SERVICE_URL` 或 `OVERTURE_BUILDING_ENDPOINT` 配置。

## 打包

```powershell
npm run bundle:v1
```

安装包输出到 `artifacts/installer/`。安装包只包含三个 release 可执行文件与第三方声明，不包含 `target/`、Rust 工具链、Node.js、Python、模型权重或构建缓存。

## 代码布局

- `native/apps/campus-native`：Slint 主程序与唯一应用状态入口。
- `native/apps/campus-map`：独立 WebView2 高德工具。
- `native/apps/campus-preview`：独立 wgpu 原生预览。
- `native/crates/campus-state`：项目、自动保存、兼容导入、撤销/重做。
- `native/crates/campus-services`：OSM 与可选 Overture 查询。
- `native/crates/campus-export`：Sponge V3 `.schem`。
- `src-tauri/crates/arnis-core`：固定上游版本的 Arnis 建筑规则移植。
- `services/`：开发/云端 Overture 查询服务。
- `src/`：旧网页兼容实现，非 V1 桌面运行时依赖。
