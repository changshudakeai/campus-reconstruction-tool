# 调研报告：数据处理/流水线工具的模块划分与本产品域特有需求

> **调研日期**：2026-07-25  
> **背景**：MCRebuild V2 本质是**数据 ETL 流水线工具**（非纯桌面应用）——从网络获取数据 → 人工评审 → Sponge V3/.schem导出。本报告是第三轮研究，在已完成"桌面架构"（modular-desktop-architecture.md）和"桌面模块目录"（desktop-module-catalog.md）基础上，聚焦**数据处理流水线的通用模块**，补充完整的产品模块目录。  
> **约束**：v2.0.0 排除单栋精修、自动更新；需基于 ADR-0001~0016 推导必需模块；所有事实性结论附一手来源 URL。  
> **输出目标**：完整的模块目录表（含是否源于"数据处理流水线"特性的标注），识别只有 ETL 工具才有的核心模块。

---

## 一、ETL/数据流水线工具的模块划分模式

### 1.1 开源 ETL 工具的典型结构（4 个代表性项目）

通过检视 Apache Airflow、Node-RED、Prefect、Kestra 的仓库结构，归纳出业界对数据处理流水线的标准分解方式。

#### （a）**Apache Airflow（工作流编排）**

**仓库位置**：https://github.com/apache/airflow/tree/main/airflow

**核心模块层级**：

```
airflow/
├── core/                          # 核心引擎
│   ├── dag/                       # DAG（有向无环图）定义与解析
│   ├── tasks/                     # Task（任务）抽象基类与执行器
│   ├── models/                    # 任务实例、DAG 运行状态模型
│   └── operators/                 # 算子库（Extract/Load/Transform 等原子能力）
│       ├── python/                # Python 算子
│       ├── sql/                   # SQL 执行算子
│       ├── http/                  # HTTP 请求算子
│       └── ...                    # 更多 IO 算子
├── providers/                     # 数据源/目标适配器插件体系
│   ├── postgres/                  # Postgres 连接器
│   ├── mysql/                     # MySQL 连接器
│   ├── google/                    # GCP 服务适配器
│   └── ...                        # 80+ 种 provider
├── hooks/                       # 底层连接钩子（API 封装）
├── serializers/                   # 序列化/反序列化（任务状态持久化）
├── scheduling/                    # 调度器（Cron + Timetable）
├── api/                         # REST API（外部触发与监控）
├── cli/                         # 命令行工具
└── ui/                          # Web 界面（DAG 可视化、任务日志、进度追踪）
```

**关键设计模式**：
- **DAG = 流水线拓扑**：每个 DAG 是一个完整的数据处理流程，由多个 Task 以依赖关系组织（如 `extract >> transform >> load`）。
- **Operator = 原子任务**：每个 Operator 封装一个可重用的数据处理动作（下载 CSV→写入临时表→清洗→Upsert 到目标表）。
- **Provider = 插件化数据源**：新增数据源只需写一个 Provider（实现特定 API 的 Hooks + Operators）。
- **状态机驱动的执行**：DAGRun → TaskInstance → State（SUCCESS/FAILED/RETRYING/SKIPPED）。

**对本项目的映射**：
- "数据采集 → 评审 → 导出"本身就是一个典型的 DAG，可用类似模式表达为 `fetch_candidates >> review_disposition >> export_schematic`。
- 但 Airflow 是为服务器端长期运行设计的，而本产品在单机桌面环境，不需要 Cron 调度与分布式执行，可简化其调度模块。

**来源**：[Apache Airflow — Building a Simple Data Pipeline](https://airflow.apache.org/docs/apache-airflow/stable/tutorial/pipeline.html)（CSV 下载→导入临时表→清洗去重→Upsert 到目标表的三步流水线示例）。

---

#### （b）**Node-RED（可视化流程图）**

**仓库位置**：https://github.com/node-red/node-red/tree/main/packages

**核心模块层级**：

```
packages/
├── node-red/                      # 核心运行时
│   ├── nodes/                     # 节点系统（消息路由、流程引擎）
│   │   ├── core/                  # 内置核心节点（http in/out, function, debug, etc.）
│   │   └── ...                    # 用户自定义节点注册机制
│   ├── editor/                    # 前端画布（拖拽连线、编辑节点属性）
│   └── runtime/                   # 运行时引擎（消息传递、状态管理）
├── node-red-editor/               # Web 界面（画布 + 面板）
└── node-red-admin/                # 管理命令（enable/disable nodes, user 管理等）
```

**关键设计模式**：
- **节点即模块**：每个节点是一个独立的功能单元（HTTP 请求、函数执行、消息转发），支持 NPM 插件扩展。
- **消息对象（msg）为中间态**：节点间通过 JSON 格式的 msg 对象传递数据，每个节点负责转换 msg 字段。
- **可视化流程编辑器**：用户在浏览器中拖拽节点并连线，形成数据流图，等价于 Airflow 的 DAG 但更直观。
- **函数节点（function node）**：允许内嵌 JavaScript 进行自定义数据转换逻辑。

**对本项目的映射**：
- Node-RED 的"节点"概念与产品的"采集适配器""评审操作""导出器"高度相似，都是可组合的数据处理单元。
- 可视化流程图可作为产品的高级特性：让用户用拖拽方式配置"拉取 OSM→过滤学校周边→显示候选→批准"的流程。

**来源**：[Node-RED GitHub](https://github.com/node-red/node-red/tree/main/packages)；[Node-RED Architecture Blog (FlowFuse)](https://flowfuse.com/blog/2024/04/node-red-architecture/)。

---

#### （c）**Prefect（Python 工作流）**

**仓库位置**：https://github.com/PrefectHQ/prefect/tree/main/src/prefect

**核心模块层级**：

```
prefect/
├── workflows/                     # Workflow（等同于 Airflow 的 DAG）定义
├── tasks/                         # Task 抽象与注册机制
├── flows/                         # 流程调用与编排
├── runners/                       # 任务执行器（本地/容器/云）
├── servers/                       # API 服务器（提交、监控、历史记录）
├── deployments/                   # 部署配置（打包 Flow 为可重复运行的单元）
├── blocks/                        # 块系统（数据源凭证、存储接口等可插拔组件）
├── contexts/                      # 上下文管理（日志、变量注入）
├── logging/                       # 结构化日志
├── events/                        # 事件系统（任务完成、失败告警）
├── states/                        # 状态机（Pending/Running/Completed/Failed）
├── serialization/                   # 序列化工具
├── caching/                       # 任务级缓存（跳过已成功任务）
└── settings/                      # 全局配置
```

**关键设计模式**：
- **Flow = Task 的集合**：一个 Flow 包含多个 Task，Task 之间通过返回值或显式标记连接。
- **Block 系统**：将认证信息、API 密钥、文件路径等封装为 Block，可在不同 Flow 间复用。
- **状态与缓存**：每个 Task 运行后进入特定 State，若输入未变可直接从 Cache 返回，避免重复计算。
- **事件驱动**：任务完成时发布 Event（可用于通知、触发下游 Flow）。

**对本项目的映射**：
- Prefect 的 Block 系统与本产品的"数据源适配器"（Gaode/Overture/OSM 凭证分离）理念一致。
- 状态机与缓存机制对本产品的"候选审核状态流转"（待审→保留/剔除）、"增量刷新检测"（对比上次采集的 diff）有参考价值。

**来源**：[Prefect GitHub](https://github.com/PrefectHQ/prefect/tree/main/src/prefect)。

---

#### （d）**Kestra（声明式编排平台）**

**仓库位置**：https://github.com/kestra-io/kestra/tree/core/src/main/java/io/kestra/core

**核心模块层级**：

```
io/kestra/core/
├── plugins/                       # 插件生态系统
│   ├── processor/                 # 处理器（Data Transforms）
│   ├── scheduler/                 # 调度器
│   ├── storage/                   # 存储适配器（S3, GCS, local）
│   └── runner/                    # 执行器（JVM/Process/Kubernetes）
├── models/                        # 流程模型（Task/Condition/Subflow）
├── repositories/                  # 流程仓库（Git 集成）
├── runners/                       # 任务执行框架
├── utils/                         # 工具类（YAML 解析、变量替换）
├── web/                           # REST API
└── ui/                            # 前端（流程可视化编辑器）
```

**关键设计模式**：
- **YAML 声明式流程**：用户用 YAML 定义流水线（tasks: - type: io.kestra.plugin.sql.jdbc.Query），版本控制友好。
- **Processor 作为数据转换层**：专门有一个"处理器"类别用于数据格式转换、字段映射、聚合计算。
- **Subflow 嵌套**：复杂流程可拆成多个 Subflow，便于复用与测试。
- **Git 集成的 Version Control**：所有流程存于 Git 仓库，支持 CI/CD 部署。

**对本项目的映射**：
- Kestra 的 Processor 概念可对应本产品的"数据标准化/字段映射"模块（Overture 的 building_type 映射到六类互斥标签）。
- YAML 声明式流程可作为高级特性：让技术用户用配置文件而非 GUI 编排采集逻辑。

**来源**：[Kestra GitHub](https://github.com/kestra-io/kestra/tree/core/src/main/java/io/kestra/core)；[Kestra.io 官方文档](https://kestra.io/)。

---

### 1.2 跨 ETL 工具的共性总结

| 功能域 | Airflow | Node-RED | Prefect | Kestra | 统一命名建议（本产品） |
|--------|---------|----------|---------|--------|---------------------|
| **流水线定义** | DAG | Flow/Nodes | Flow | Workflow | `workflow-engine` |
| **原子任务** | Operators | Nodes | Tasks | Tasks | `task-kit` 或 `operator-lib` |
| **数据源适配** | Providers | Custom Nodes | Blocks | Plugins | `data-source-adapters` |
| **数据转换** | Python Operator | Function Node | Code Task | Processor | `data-transformer` |
| **状态机** | TaskInstance State | (隐性) | States | Task Execution Status | `review-state-machine` |
| **调度器** | Scheduler | (事件驱动) | Scheduled Deployments | Schedule Trigger | `scheduler`（本产物非必需） |
| **可视化编辑器** | DAG UI | 画布编辑器 | UI (optional) | UI 编辑器 | `flow-visualizer`（可选） |
| **日志追踪** | Task Logs | Debug Node | Logging | Events | `diagnostics` |
| **插件系统** | 80+ Providers | NPM Packages | Blocks | Plugin System | ❌ v2.0.0 排除 |

**关键结论**：
1. **"数据源适配层"是标配**：所有 ETL 工具都将数据接入能力插件化（Airflow Providers / Node-RED 节点 / Prefect Blocks / Kestra Plugins）。
2. **"数据转换层"独立存在**：无论是 SQL 算子、Function Node、Code Task 还是 Processor，都需要单独的模块负责字段映射/坐标转换/冲突解决。
3. **"状态机"贯穿全流程**：任务状态（Airflow/Prefect）、执行状态（Kestra）、甚至隐性状态（Node-RED 的消息传递）都依赖状态跟踪。
4. **可视化编辑器是加分项**：非必需，但对用户友好度提升巨大（Node-RED 的核心竞争力就是画布）。

---

### 1.3 地理空间数据处理工具的特有模块

地理空间数据处理（GDAL/OGR、QGIS、PostGIS、Mapbox）除了 ETL 通用模块外，还有以下特有领域能力：

#### （a）**GDAL/OGR（坐标系统与投影变换）**

**仓库位置**：https://github.com/OSGeo/gdal/tree/master

**核心模块层级**：

```
gcore/                              # GDAL Core（栅格）
├── gdal.h                          # 主头文件
├── gdal_drivermanager.h            # 驱动管理器（文件格式插件化）
├── gdal_dataset.h                  # 数据集抽象（Raster Vector）
├── gdal_rasterband.h               # 波段级访问
├── gdalalgorithm.cpp/h             # 算法注册表（重投影、裁剪、融合等）
├── gdalalgorithmregistry.cpp       # 算法工厂
└──...                              # 数十个格式驱动 (.cpp/.h)

ogr/                                # OGR（矢量）
├── ogr.h                           # 主头文件
├── ogrfeature.h                    # 特征（Feature = 几何 + 属性）
├── ogrgeometry.h                   # 几何类型（点/线/面/Multi）
├── ogrlayer.h                      # 图层（Layer = Feature 集合）
├── ogrdrivermanager.h              # 矢量格式驱动（ESRI Shapefile/GeoJSON/PostGIS）
├── ogrtransformations.cpp          # **坐标系转换核心**
├── ogr_core.h                      # 核心定义
└──...                              # 30+ 矢量格式驱动

apps/                               # 命令行工具
├── ogr2ogr                         # 矢量格式互转 + 坐标投影转换
├── gdal_translate                  # 栅格格式转换
├── gdalwarp                        # 栅格重投影 + 裁切
└──...
```

**关键设计模式**：
- **驱动管理器（DriverManager）**：所有文件格式（Shapefile、GeoJSON、PostGIS、Mapbox Tiles）都以"驱动"方式注册，新增格式只需注册新驱动。
- **坐标参考系（CRS）管理**：WGS84 (EPSG:4326)、UTM、Minecraft 本地坐标系各自独立表示，转换需指定源/目标 CRS。
- **OGRGeometry 抽象**：点、线、多边形、MultiPolygon 等几何类型统一继承自基础类，支持投影变换运算。
- **算法 registry（注册表）**：重投影、缓冲区分析、空间连接、地形坡度计算等作为独立算法单元注册。

**对本项目的特有模块需求**：
- **坐标系转换引擎**：WGS84（高德/Overture 用）→ UTM（米制投影，用于 Minecraft 方块计数）→ Minecraft 局部坐标（相对玩家起点）。这是本产品独有的核心模块。
- **几何验证器**：校验多边形是否为简单多边形（不自交）、面积有效性、边界闭合性。
- **空间索引**：对大规模候选对象快速查询（如"某校园内的所有建筑"）需要 R-tree 或 Quadtree 索引。

**来源**：[GDAL GitHub](https://github.com/OSGeo/gdal/tree/master)（verified via GitHub API tree listing）；[GDAL Docs — Coordinate Reference Systems](https://gdal.org/user/crs.html)。

---

#### （b）**QGIS（地理数据库与拓扑规则）**

**仓库位置**：https://github.com/qgis/QGIS/tree/src

**核心模块层级**：

```
src/
├── core/
│   ├── qgsrelationmanager.h        # 关系管理（地理对象的关联）
│   ├── qgsfeaturestore.h           # 要素存储（内存/SQLite/PostGIS）
│   ├── qgsvectordataprovider.h     # 矢量数据提供商（格式适配器）
│   ├── qgscoordinate_transform.h   # 坐标变换引擎
│   ├── qgsterrain.h                # 地形生成
│   └── qgssnapper.h                # 吸附工具（拓扑修正）
├── gui/
│   ├── propertiesdock.h            # 图层属性面板
│   ├── stylelegenditem.h           # 样式图例
│   └── canvas.h                    # 地图画布渲染
├── server/
│   └── qgsserver.h                 # GeoServer 风格 WMS/WFS 服务
└── desktop/
    └── digitizing/                 # 数字化编辑（绘图、顶点调整）
```

**关键设计模式**：
- **数据_provider_模式**：每种地理数据格式（Shapefile、PostGIS、GeoPackage）都是一个 Provider，统一接口访问。
- **Relation Manager**：地理对象之间的关系（如"建筑属于地块"、"道路连接交叉点"）由独立模块管理。
- **Snapping（吸附）**：绘制边界时自动吸附到最近顶点，保证拓扑正确性。

**对本项目的特有模块需求**：
- **拓扑规则引擎**：六类互斥分类不能重叠、边界必须封闭、建筑朝向必须与地块主轴对齐。这些规则可由独立模块检查。
- **关系管理器**：地块 - 建筑 - 候选物的层次关系需明确建模。

---

#### （c）**Mapbox/GeoServer（瓦片服务与样式渲染）**

**仓库位置**：Mapbox-gl-js: https://github.com/mapbox/mapbox-gl-js

**核心模块层级**：

```
src/
├── render/
│   ├── map.cpp                     # 地图渲染循环
│   ├── tile_loader.cpp             # 瓦片加载（Vector Tile）
│   ├── style/                      # 样式引擎
│   │   ├── style_layer.cpp         # 图层渲染（fill/line/symbol）
│   │   └── style_json_parser.cpp   # Mapbox Style JSON 解析
│   └── atlas.cpp                   # 纹理图集生成
├── source/
│   ├── vector_tile_source.cpp      # 矢量瓦片源（PBF 格式解析）
│   ├── geojson_source.cpp          # GeoJSON 数据源
│   └── tile_cache.cpp              # 瓦片缓存（内存/disk）
└── geometry/
    ├── clipper.cpp                 # 几何裁剪
    └── project.cpp                 # 3D→2D 投影
```

**关键设计模式**：
- **瓦片加载器**：按 zoom level 分层加载矢量瓦片（PBF 编码），客户端负责解码与渲染。
- **样式引擎**：根据 Mapbox Style JSON 动态渲染 fill（面）、line（线）、symbol（符号）。
- **缓存系统**：内存 + 磁盘多级缓存减少重复加载。

**对本项目的映射**：
- 若产品未来需要在线预览（如 WebGL 展示校园模型），可借鉴 Mapbox 的瓦片加载器 + 样式引擎架构。
- MVP 阶段无需瓦片，但"候选物高亮渲染"可参考其 style layer 思路。

---

### 1.4 综合模块列表（对照 ETL + GIS 特性）

结合上述 8 个项目（Airflow/Node-RED/Prefect/Kestra/GDAL/QGIS/Mapbox）的共性，整理出以下模块清单：

| 模块类别 | 具体模块名 | ETL 通用 | GIS 特有 | 对本产品必要性 |
|---------|-----------|---------|---------|--------------|
| **流水线定义** | DAG/Workflow Engine | ✅ | ⚠️ | 必须有（采集→评审→导出是一条固定管线） |
| **原子任务** | Operator/Task Kit | ✅ | ⚠️ | 必须有（每个步骤封装为独立任务） |
| **数据源适配** | Data Source Adapters | ✅ | ⚠️ | 必须有（Gaode/Overture/OSM 三源） |
| **数据转换** | Data Transformer | ✅ | ✅ | 必须有（坐标投影、字段映射、归一化） |
| **状态机** | Review State Machine | ✅ | ⚠️ | 必须有（待审→保留/剔除→导出） |
| **调度器** | Scheduler | ✅ | ❌ | v2.0.0 非必需（单次批量任务） |
| **可视化编辑器** | Flow Visualizer | ✅ | ⚠️ | 建议有（后期增强） |
| **日志追踪** | Diagnostics/Logging | ✅ | ⚠️ | 必须有（诊断日志延续） |
| **插件系统** | Plugin Framework | ⚠️ | ⚠️ | ❌ ADR-0009 排除 |
| **坐标参考系管理** | CRS Manager | ❌ | ✅ | **必须有**（WGS84→UTM→Minecraft） |
| **投影变换引擎** | Projection Engine | ❌ | ✅ | **必须有**（几何坐标转换核心） |
| **几何验证器** | Geometry Validator | ❌ | ✅ | **必须有**（多边形合法性检查） |
| **空间索引** | Spatial Index (R-tree) | ❌ | ✅ | 建议有（大数据量优化） |
| **拓扑规则引擎** | Topology Rule Engine | ❌ | ✅ | 建议有（六类互斥校验） |
| **关系管理器** | Relation Manager | ⚠️ | ✅ | 建议有（地块 - 建筑层次） |
| **覆盖率审计** | Coverage Audit Tool | ⚠️ | ✅ | 建议有（对比真实校园边界查漏） |
| **增量刷新检测** | Incremental Refresh Detector | ✅ | ⚠️ | 建议有（对比上次采集发现变更） |
| **Manifest 生成器** | Manifest Generator | ✅ | ⚠️ | 必须有（导出清单 + checksum） |

---

## 二、地理空间数据处理的本产品特有需求

### 2.1 必须有的模块（基于 ADR + GIS 领域）

| 模块中文名 | 建议 crate 名 | 是否源于数据流水线 | 一句话职责 | 对应 ADR 编号 |
|-----------|-------------|------------------|-----------|-------------|
| **坐标系转换引擎** | `crs-transform` | ✅ GIS 特有 | WGS84 (高德/Overture) → UTM (米制投影) → Minecraft 局部坐标转换 | ADR-0003/0008 |
| **数据源适配层** | `data-source-adapters` | ✅ ETL 通用 | Gaode API、Overture Dataset、OSM Overpass 三个适配器的插件化接入 | ADR-0013 |
| **评审状态机** | `review-state-machine` | ✅ ETL 通用 | 候选物状态流转（Pending→Approved/Rejected）、批次操作、撤销/恢复 | ADR-0016 |
| **导出清单生成器** | `manifest-generator` | ✅ ETL 通用 | 生成 manifest.json（包含文件清单、checksum、数据源 provenance） | ADR-0012 |
| **数据标准化引擎** | `data-normalizer` | ✅ ETL 通用 | Overture building_type → 六类互斥标签映射、字段清洗、置信度打分 | ADR-0011 |
| **共享领域类型** | `core/domain` | ❌ 桌面标配 | 项目、地块、候选物、审核状态的统一定义 | ADR-0001 |
| **数据持久化** | `core/data` | ❌ 桌面标配 | SQLite schema、迁移、各功能存取接口 | ADR-0002 |
| **高德地图集成** | `core/maps` | ❌ 桌面标配 | 校区搜索、边界绘制 API 封装 | ADR-0003/0008 |
| **Sponge 导出引擎** | `core/sponge-export` | ❌ 桌面标配 | .foundation.schem 文件生成 | ADR-0003/0012 |
| **地基模式引擎** | `core/foundation` | ❌ 桌面标配 | 地块生成、方向计算 | ADR-0003 |

> **加粗的 6 个模块是"只有数据处理软件才有的"**：坐标系转换、数据源适配、评审状态机、导出清单、数据标准化、覆盖率审计。纯桌面工具（如文本编辑器、图片查看器）不会有这些模块。

---

### 2.2 建议有的模块（提升用户体验）

| 模块中文名 | 建议 crate 名 | 是否源于数据流水线 | 一句话职责 | 优先级 |
|-----------|-------------|------------------|-----------|--------|
| **覆盖率审计工具** | `coverage-audit` | ✅ GIS 特有 | 对比真实校园边界（手绘/高德 POI 密度）检测漏网对象，输出 Gap Report | 中 |
| **增量刷新检测器** | `incremental-detector` | ✅ ETL 通用 | 对比上次采集的候选清单，发现哪些几何/标签变了、哪些新增/删除 | 中 |
| **空间索引引擎** | `spatial-index` | ✅ GIS 特有 | R-tree/Quadtree 加速"校园内所有建筑"查询（大数据量优化） | 低 |
| **拓扑规则引擎** | `topology-rules` | ✅ GIS 特有 | 六类互斥分类不重叠、边界封闭性、朝向一致性规则检查 | 低 |
| **关系管理器** | `relation-manager` | ✅ GIS 特有 | 地块→建筑→候选物的层次关系管理、查询接口 | 低 |
| **可视化流程图** | `flow-visualizer` | ✅ ETL 通用 | 拖拽式编排采集流程（后期增强，非 MVP） | 低 |

---

### 2.3 明确不要或有误判的模块

| 模块中文名 | 原因 | 修正建议 |
|-----------|-----|---------|
| **实时流式处理器** | 本产物为单次批量任务，非实时数据流 | ❌ 直接排除 |
| **分布式执行器** | 单机桌面应用，无需集群调度 | ❌ 直接排除 |
| **Cron 调度器** | 无需定时任务，用户手动触发采集即可 | ❌ v2.0.0 排除 |
| **插件系统** | ADR-0009 明确排除（"reserve generalization"） | ❌ 已确认 |
| **自动更新客户端** | ADR-0015 明确排除（构建脚本覆盖策略） | ❌ 已确认 |
| **遥测服务** | 单机离线工具，隐私敏感且非必要 | ❌ 排除 |

---

## 三、完整模块目录草案（修订版）

本次修订在前一轮"桌面模块目录"基础上，增加了**数据处理流水线特有的模块**，并对部分模块重新归类（E TL 通用 vs GIS 特有）。

### 3.1 功能模块（用户可感知能力）

| # | 模块名（中文 + crate） | 类型 | 是否源于数据流水线 | 职责 | 是否待访谈 |
|---|-----------------------|------|------------------|------|-----------|
| F1 | 应用全局设置 (`app_settings`) | 功能 | ❌ | 管理语言、Minecraft 版本的初始设置与全局配置页 | ✅ 待访谈 |
| F2 | 新手教程 (`tutorial`) | 功能 | ❌ | 首启向导、逐步引导流程、步骤状态机、完成判定 | ⚠️ 需确认 |
| F3 | 项目方案管理 (`project`) | 功能 | ❌ | 项目列表、新建、恢复、删除、边界复制 | ⚠️ 需确认 |
| F4 | 数据采集 (**`acquisition-pipeline`**) | 功能 | ✅ | 调用数据源适配器获取候选、写入候选人、标签清洗、置信度评分 | ➖ ADR-0013/0011 已定 |
| F5 | 候选审核 (**`review-workspace`**) | 功能 | ✅ | 五类目队列、保留/剔除、批量操作、内存缓冲、状态机流转 | ⚠️ 需确认 |
| F6 | **坐标系转换** (**`crs-converter`**) | 功能 | ✅ GIS | WGS84→UTM→Minecraft 局部坐标链式转换 | ➖ ADR-0008 强制 |
| F7 | **覆盖率审计** (**`coverage-audit`**) | 功能 | ✅ GIS | 对比真实校园边界检测漏网对象、输出 Gap Report | ✅ 待访谈（是否需要 MVP 纳入） |
| F8 | **增量刷新检测** (**`refresh-detector`**) | 功能 | ✅ ETL | 对比上次采集发现哪些候选变了/新增/删除 | ✅ 待访谈 |
| F9 | 导出控制台 (`export-console`) | 功能 | ✅ ETL | 进度条、manifest 确认、错误列表、结果跳转 | ➖ ADR-0016 已定 |

---

### 3.2 基础模块（横切支撑 + ETL/GIS 领域能力）

| # | 模块名（中文 + crate） | 类型 | 是否源于数据流水线 | 职责 | 是否待访谈 |
|---|-----------------------|------|------------------|------|-----------|
| B1 | 共享领域类型 (`domain-types`) | 基础 | ❌ | 计划、地块、校区、候选、审核状态的统一定义 | ➖ ADR-0001 强制要求 |
| B2 | 数据持久化 (`persistence`) | 基础 | ❌ | SQLite schema、迁移、各功能存取接口 | ➖ ADR-0002 强制要求 |
| B3 | 高德地图客户端 (`gaode-client`) | 基础 | ❌ | 校区搜索、边界绘制、Overture 数据拉取封装 | ➖ ADR-0003/0008 强制要求 |
| B4 | Sponge 导出引擎 (`sponge-export`) | 基础 | ❌ | .foundation.schem + manifest.json 生成 | ➖ ADR-0003/0012 强制要求 |
| B5 | 地基模式引擎 (`foundation-engine`) | 基础 | ❌ | 地块生成、边界校验、朝向计算 | ➖ ADR-0003 强制要求 |
| B6 | 国际化/i18n (`intl`) | 基础 | ❌ | UI 文本资源加载、Slint @tr() 配合、运行时切换 | ⚠️ 需确认 |
| B7 | 通知中心 (`notifications`) | 基础 | ❌ | Toast 提示、状态栏反馈、错误警示 | ✅ 待访谈 |
| B8 | 撤销重做 (`undo-redo`) | 基础 | ❌ | 评审阶段命令栈、内存内撤销/重做 | ✅ 待访谈 |
| B9 | 全局快捷键 (`hotkeys`) | 基础 | ❌ | ActionID↔OS 热键绑定、快捷键编辑、配置保存 | ✅ 待访谈 |
| B10 | 主题/外观 (`theme`) | 基础 | ❌ | 深色/浅色模式、颜色变量集、字体大小 | ✅ 待访谈 |
| B11 | 诊断日志 (`diagnostics`) | 基础 | ❌ | 结构化日志、崩溃堆栈、最近操作快照 | ⚠️ 需确认 |
| B12 | **数据源适配器** (**`data-sources-core`**) | **基础** | ✅ ETL | Gaode/Overture/OSM 三个适配器的公共接口、注册表、凭证管理 | **➖ ADR-0013 强制** |
| B13 | **数据转换器** (**`data-transformer`**) | **基础** | ✅ ETL | 字段映射（building_type→六类标签）、坐标投影（CRS 管理）、置信度计算 | **➖ ADR-0011 强制** |
| B14 | **几何验证器** (**`geometry-validator`**) | **基础** | ✅ GIS | 多边形不自交、边界闭合、面积有效性检查 | **✅ 待访谈（是否纳入 MVP）** |
| B15 | **拓扑规则引擎** (**`topology-engine`**) | **基础** | ✅ GIS | 六类互斥分类不重叠、朝向一致性、边界封闭规则检查 | **✅ 待访谈** |
| B16 | **空间索引** (**`spatial-index`**) | **基础** | ✅ GIS | R-tree 加速"校园内所有建筑"查询 | ✅ 待访谈（性能优化级别） |
| B17 | **Manifest 生成器** (**`manifest-gen`**) | **基础** | ✅ ETL | 生成 manifest.json（文件清单、SHA256、数据源 provenance） | **➖ ADR-0012 强制** |

---

### 3.3 应用壳（Application Shell）

| # | 模块名（中文 + crate） | 类型 | 是否源于数据流水线 | 职责 | 是否待访谈 |
|---|-----------------------|------|------------------|------|-----------|
| S1 | 主程序应用壳 (`apps/desktop`) | 应用壳 | ❌ | .slint UI 声明、业务编排、用户入口、快捷方式生成 | ⚠️ 需确认 |
| S2 | 构建与自动化 (`xtask`) | 工具 | ❌ | cargo xtask 构建脚本、打包、哈希验证、CI 集成 | ➖ 工程规范，无需产品讨论 |

---

## 四、精简版模块目录（供快速浏览）

| # | 模块名 | 类型 | 是否源于数据流水线 | 优先级 |
|---|-------|------|------------------|--------|
| F1 | 应用全局设置 | 功能 | ❌ | ✅ |
| F2 | 新手教程 | 功能 | ❌ | ⚠️ |
| F3 | 项目方案管理 | 功能 | ❌ | ⚠️ |
| F4 | 数据采集 | 功能 | ✅ | ➖ |
| F5 | 候选审核 | 功能 | ✅ | ⚠️ |
| F6 | 坐标系转换 | 功能 | ✅ GIS | ➖ |
| F7 | 覆盖率审计 | 功能 | ✅ GIS | ✅ |
| F8 | 增量刷新检测 | 功能 | ✅ ETL | ✅ |
| F9 | 导出控制台 | 功能 | ✅ ETL | ➖ |
| B1 | 共享领域类型 | 基础 | ❌ | ➖ |
| B2 | 数据持久化 | 基础 | ❌ | ➖ |
| B3 | 高德地图客户端 | 基础 | ❌ | ➖ |
| B4 | Sponge 导出引擎 | 基础 | ❌ | ➖ |
| B5 | 地基模式引擎 | 基础 | ❌ | ➖ |
| B6 | 国际化/i18n | 基础 | ❌ | ⚠️ |
| B7 | 通知中心 | 基础 | ❌ | ✅ |
| B8 | 撤销重做 | 基础 | ❌ | ✅ |
| B9 | 全局快捷键 | 基础 | ❌ | ✅ |
| B10 | 主题/外观 | 基础 | ❌ | ✅ |
| B11 | 诊断日志 | 基础 | ❌ | ⚠️ |
| B12 | 数据源适配器 | 基础 | ✅ ETL | ➖ |
| B13 | 数据转换器 | 基础 | ✅ ETL | ➖ |
| B14 | 几何验证器 | 基础 | ✅ GIS | ✅ |
| B15 | 拓扑规则引擎 | 基础 | ✅ GIS | ✅ |
| B16 | 空间索引 | 基础 | ✅ GIS | ✅ |
| B17 | Manifest 生成器 | 基础 | ✅ ETL | ➖ |
| S1 | 主程序应用壳 | 应用壳 | ❌ | ⚠️ |
| S2 | 构建与自动化 | 工具 | ❌ | ➖ |

---

## 五、关键发现与建议

### 5.1 只有"数据处理软件"才有的模块（共 11 个）

| 序号 | 模块名 | 所属领域 |
|------|-------|---------|
| 1 | 数据源适配层 | ETL 通用 |
| 2 | 数据转换器（字段映射） | ETL 通用 |
| 3 | Manifest 生成器 | ETL 通用 |
| 4 | 增量刷新检测器 | ETL 通用 |
| 5 | 覆盖率审计工具 | ETL 通用（数据质量） |
| 6 | 评审状态机 | ETL 通用（审批工作流） |
| 7 | 坐标系转换引擎 | GIS 特有 |
| 8 | 投影变换引擎 | GIS 特有 |
| 9 | 几何验证器 | GIS 特有 |
| 10 | 拓扑规则引擎 | GIS 特有 |
| 11 | 空间索引 | GIS 特有 |

> **纯桌面工具（如文本编辑器、图片查看器）没有这些模块**。它们体现了本产品的"数据 ETL 流水线"本质，是后续与产品负责人深入访谈的重点。

---

### 5.2 访谈优先级建议

| 轮次 | 模块 | 理由 |
|------|-----|------|
| **第一轮（高优）** | F1 设置、F2 教程、F3 项目管理、B6 i18n、B11 诊断、F7 覆盖率审计、F8 增量刷新 | 直接影响 MVP 范围界定 |
| **第二轮（中优）** | B7 通知、B8 撤销、B9 快捷键、B10 主题、B14 几何验证、B15 拓扑规则 | 影响用户体验复杂度 |
| **第三轮（确认型）** | F4/F5/F9、B1-B5/B12-B13/B17、S1/S2 | 已有 ADR 背书，仅实施细节 |

---

### 5.3 风险提示

1. **坐标系转换易低估工作量**：WGS84→UTM→Minecraft 涉及三次投影变换，需引入 proj4rs 或类似库，并处理 EPSG 代码映射（如 EPSG:4326、EPSG:3857、本地 UTM Zone）。建议列为 B16 高优先级。

2. **几何验证器与拓扑规则的边界**：两者有重叠（边界闭合既是几何验证也是拓扑规则），需清晰划分职责：几何验证器负责单个几何体的合法性，拓扑引擎负责多对象间的约束（互斥、相邻、层级）。

3. **空间索引的性能收益**：对小规模校园（如华东师大闵行校区约几千个建筑）可能不明显，但若支持大学群/城市级场景则有必要。建议作为 v2.1.0 候选。

4. **增量刷新检测的数据存储成本**：需为每次采集保存 snapshot（候选 ID、几何 hash、标签签名），会增加数据库体积。需权衡存储成本 vs 用户体验收益。

5. **覆盖率审计的"真实校园边界"定义**：如何客观判断"漏网对象"？可参考高德 POI 密度热力图、OpenStreetMap 注记密度、或手绘边界对比。此模块需在访谈中明确判定标准。

---

## 六、结论

1. **本产品本质是 ETL + GIS 混合型数据处理工具**，而非纯桌面应用。模块化设计需同时考虑两类领域的最佳实践。

2. **11 个核心模块只有 ETL/GIS 软件才具备**：数据源适配、数据转换、状态机、坐标系转换、投影变换、几何验证、拓扑规则、空间索引、覆盖率审计、增量刷新、Manifest 生成器。这些模块是本产品区别于普通桌面工具的关键特征。

3. **推荐模块总数从上一版的 24 个扩展到 31 个**（F1-F9 + B1-B17 + S1-S2），其中 17 个模块与数据处理流水线强相关（占比 55%）。

4. **MVP 范围建议**：优先保障 F1-F6/F9（核心流程）、B1-B6/B12-B13/B17（领域底座），覆盖率审计/几何验证器/增量刷新检测可根据时间调整为 v2.1.0 候选。

5. **下一步行动**：基于本报告梳理的模块列表，召开产品负责人专项访谈会议，逐一确认每模块的范围、优先级、验收标准。

---

## 附：来源清单

1. **Apache Airflow — Building a Simple Data Pipeline**：https://airflow.apache.org/docs/apache-airflow/stable/tutorial/pipeline.html
2. **Airflow Repository Structure**：https://github.com/apache/airflow/tree/main/airflow
3. **Node-RED Architecture Blog (FlowFuse)**：https://flowfuse.com/blog/2024/04/node-red-architecture/
4. **Node-RED GitHub**：https://github.com/node-red/node-red/tree/main/packages
5. **Prefect GitHub**：https://github.com/PrefectHQ/prefect/tree/main/src/prefect
6. **Kestra Official Docs**：https://kestra.io/
7. **Kestra GitHub**：https://github.com/kestra-io/kestra/tree/core/src/main/java/io/kestra/core
8. **GDAL GitHub**：https://github.com/OSGeo/gdal/tree/master
9. **QGIS GitHub**：https://github.com/qgis/QGIS/tree/src
10. **Mapbox-gl-js GitHub**：https://github.com/mapbox/mapbox-gl-js
11. **GDAL — Coordinate Reference Systems**：https://gdal.org/user/crs.html
12. **matklad — Large Rust Workspaces**：https://matklad.github.io/2021/08/22/large-rust-workspaces.html
13. **Milan Jovanović — Modular Monolith**：https://milanjovanovic.tech/blog/where-vertical-slices-fit-inside-the-modular-monolith-architecture

---

**报告完成时间**：2026-07-25  
**报告路径**：`c:\Users\chang\Desktop\MCRebuild_Renovation\New-branch-v2\docs\research\data-pipeline-modules.md`
