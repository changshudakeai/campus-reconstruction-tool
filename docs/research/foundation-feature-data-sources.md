# Foundation feature data-source research

Date: 2026-07-14
Overture snapshot checked: `2026-06-17.0`, schema `v1.17.0` (the latest release on the research date; the next release was scheduled for 2026-07-22). Source: [Overture release calendar](https://docs.overturemaps.org/release-calendar/).

## Question

OSM/Overture 是否只能查询建筑？Campus Reconstruction Tool V1.1 应如何确定建筑、道路/通行、水域、植被和体育设施五类 Foundation 地物？

## Short answer

不是。**Overture 数据本身不只包含建筑**：

- `buildings/building`、`building_part`：建筑面；
- `transportation/segment`、`connector`：道路、步道、台阶等中心线与连接关系；
- `base/water`：河流、溪流、运河、湖泊、池塘等点/线/面；
- `base/land`、`land_use`、`land_cover`：树、树列、林地、草地、公园用地、体育场地等点/线/面或覆盖分类。

官方入口分别见 [Buildings Guide](https://docs.overturemaps.org/guides/buildings/)、[Transportation Guide](https://docs.overturemaps.org/guides/transportation/)、[Base Guide](https://docs.overturemaps.org/guides/base/)。

真正“只有建筑”的是**本仓库当前受控服务实现**：`services/README.md:13` 只公布 `/overture/buildings`，`services/overture_bridge.py:353-363` 也只接受该路径。非建筑目前由桌面端直接请求公共 Overpass，而不是由受控服务提供。因此 V1.1 的任务不是寻找一种神奇的视觉识别替代全部地物，而是先把 OSM 与 Overture 已有的结构化图层完整接入受控服务，再只对剩余缺口使用许可清晰的补充证据。

## Source matrix

| Foundation 类别 | OSM 标准对象与几何 | Overture 可用类型 | V1.1 推荐用途 |
|---|---|---|---|
| Building | `building=*` 的闭合 way 或 multipolygon relation；建筑部件为 `building:part=*` | `buildings/building`、`building_part`，Polygon/MultiPolygon | Overture 与 OSM 并行获取；按 `sources[]` 判断是否同源，再做空间 conflation；建筑不因校园边界裁剪成半栋 |
| Road / circulation | 车辆道路和校园内部道路 `highway=*`；`footway`、`path`、`cycleway`、`steps`；`highway=pedestrian + area=yes` 或 `area:highway=*` 表示广场/宽通行面 | `transportation/segment` 的 `subtype=road`，`class` 包含 `service`、`pedestrian`、`footway`、`steps`、`path`、`cycleway` 等；LineString 中心线；`connector` 为 Point | Overture 提供规范化网络与连接关系，OSM 补充原始 tag 和通行面 polygon；中心线必须结合明确宽度或带标记的样式默认值转成生成面 |
| Water | 面：`natural=water` + `water=lake/pond/reservoir/...`；线：`waterway=river/stream/canal/...`；复杂面为 multipolygon | `base/water` 支持 Point、LineString、Polygon、MultiPolygon，class/subtype 包含 river、stream、canal、lake、pond 等 | 优先采用有效水面 polygon；只有中心线时使用 `width=*` 或明确的推断宽度；按当前产品范围排除 drain、ditch、fountain 等非目标类型 |
| Vegetation | 面：`natural=wood/scrub/grassland`、`landuse=forest/grass/meadow`；线：`natural=tree_row`；点：`natural=tree` | `base/land` 可表达 tree、tree_row、forest、grass、meadow、scrub 等；`base/land_use` 表达人类用途；`base/land_cover` 提供 forest、grass、shrub 等覆盖面 | OSM/Overture Base 的明确对象作为主数据；ESA WorldCover 衍生的 `land_cover` 只作为较粗的覆盖缺口提示，不能替代单树、树列或校园小块绿化 |
| Sports | 实际场地：`leisure=pitch + sport=*` 的 area；跑道：`leisure=track` 的 line/area；容器：`leisure=stadium/sports_centre`、`landuse=recreation_ground` | `base/land_use` class 包含 pitch、track、stadium、recreation_ground 等，几何可为点/线/面；源 tag 可保留 | 优先选择 pitch/track 的实际几何；stadium、sports_centre、recreation_ground 只作容器，不能把整个容器填成球场；体育馆建筑仍归 Building |

OSM 语义的官方说明：[Key:highway](https://wiki.openstreetmap.org/wiki/Key%3Ahighway)、[highway=footway](https://wiki.openstreetmap.org/wiki/Tag%3Ahighway%3Dfootway)、[highway=steps](https://wiki.openstreetmap.org/wiki/Tag%3Ahighway%3Dsteps)、[highway=pedestrian](https://wiki.openstreetmap.org/wiki/Tag%3Ahighway%3Dpedestrian)、[area:highway=pedestrian](https://wiki.openstreetmap.org/wiki/Tag%3Aarea%3Ahighway%3Dpedestrian)、[Key:waterway](https://wiki.openstreetmap.org/wiki/Key%3Awaterway)、[water=lake](https://wiki.openstreetmap.org/wiki/Tag%3Awater%3Dlake)、[natural=wood](https://wiki.openstreetmap.org/wiki/Tag%3Anatural%3Dwood)、[natural=tree_row](https://wiki.openstreetmap.org/wiki/Tag%3Anatural%3Dtree_row)、[natural=tree](https://wiki.openstreetmap.org/wiki/Tag%3Anatural%3Dtree)、[leisure=pitch](https://wiki.openstreetmap.org/wiki/Tag%3Aleisure%3Dpitch)、[leisure=track](https://wiki.openstreetmap.org/wiki/Tag%3Aleisure%3Dtrack)、[leisure=stadium](https://wiki.openstreetmap.org/wiki/Tag%3Aleisure%3Dstadium)、[Key:sport](https://wiki.openstreetmap.org/wiki/Key%3Asport)。

Overture schema 的直接证据：[Segment](https://docs.overturemaps.org/schema/reference/transportation/segment/)、[RoadClass](https://docs.overturemaps.org/schema/reference/transportation/types/road_class/)、[Water](https://docs.overturemaps.org/schema/reference/base/water/)、[WaterClass](https://docs.overturemaps.org/schema/reference/base/types/water_class/)、[Land](https://docs.overturemaps.org/schema/reference/base/land/)、[LandClass](https://docs.overturemaps.org/schema/reference/base/types/land_class/)、[LandUse](https://docs.overturemaps.org/schema/reference/base/land_use/)、[LandUseClass](https://docs.overturemaps.org/schema/reference/base/types/land_use_class/)、[LandCover](https://docs.overturemaps.org/schema/reference/base/land_cover/)。

## Geometry rules that matter

### Complete relations before clipping

OSM 的复杂区域不是“一条最长的线”，而是 multipolygon relation：多个 `outer` way 可以拼成一个或多个外环，`inner` way 表示洞。处理器必须先递归取得 relation 的全部成员和节点、按 role 组装有效的 Polygon/MultiPolygon，再与 Campus Boundary 求交。不能在组装前裁剪，也不能只取最长 member。Sources: [OSM multipolygon relation](https://wiki.openstreetmap.org/wiki/Relations/Multipolygon), [multipolygon processing algorithm](https://wiki.openstreetmap.org/wiki/Relation%3Amultipolygon/Algorithm), [Overpass QL `out geom` and recurse](https://wiki.openstreetmap.org/wiki/Overpass_API/Overpass_QL).

在校园边界内的推荐空间规则：

- area（water、vegetation、sports、pedestrian plaza）：保留多面与洞，随后按 Campus Boundary 裁剪；
- line（road、path、steps、tree row、linear waterway）：按边界裁剪为一段或多段，不把不相连片段硬连起来；
- point（tree 或仅有点的候选）：只保留 boundary 内的点；
- building：不裁成半栋；主体在校内则保留完整 footprint，明显跨界且归属不清则进入待审核。

### Centerline is not surface area

OSM 道路与 Overture transportation segment 通常是中心线；Overture 官方 schema 明确称 segment geometry 为 LineString centerline，并用 `width_rules` 表示宽度。OSM 可用 `width=*` / `est_width=*`，宽人行区域则可能直接由 `highway=pedestrian + area=yes` 或 `area:highway=*` 表示。Sources: [Overture segments and connectors](https://docs.overturemaps.org/guides/transportation/segments-and-connectors/), [Overture Segment schema](https://docs.overturemaps.org/schema/reference/transportation/segment/), [OSM footway width](https://wiki.openstreetmap.org/wiki/Key%3Afootway), [OSM area:highway=footway](https://wiki.openstreetmap.org/wiki/Tag%3Aarea%3Ahighway%3Dfootway).

推荐顺序为：显式 area polygon > 显式 `width` / Overture `width_rules` 缓冲中心线 > 按 road subtype 和 style pack 的版本化默认宽度。最后一种必须记录 `inferred_width=true`，降低几何置信度，不能伪装成实测宽度。

水系也有同样问题：`natural=water` 通常是水面，`waterway=river/stream/canal` 通常是流向中心线；较宽水道应有面积映射。只有线而无宽度时，可生成带“宽度为样式推断”的候选，不应声称恢复了真实岸线。Source: [OSM Key:waterway](https://wiki.openstreetmap.org/wiki/Key%3Awaterway).

### Semantic nesting must be retained

一个体育场可能同时包含 stadium 容器、pitch 面、track 环和 grandstand/building。官方 OSM 指引明确将这些作为不同对象。V1.1 应保留 `container_id` / nesting，不以重叠为由删除 pitch 或 track，也不把 sports hall 当作室外体育面。Sources: [OSM stadium](https://wiki.openstreetmap.org/wiki/Tag%3Aleisure%3Dstadium), [OSM pitch](https://wiki.openstreetmap.org/wiki/Tag%3Aleisure%3Dpitch), [OSM track](https://wiki.openstreetmap.org/wiki/Tag%3Aleisure%3Dtrack).

同理，`leisure=park` 是用途边界，不等于整块区域都是植被；植被生成应依赖 `natural=*`、`landuse=grass/forest/meadow`、单树/树列或 land-cover 证据，不能将所有 leisure/park polygon 自动分类为绿色覆盖。

## Recommended source order and merge policy

“来源顺序”不是“第一个返回就覆盖后面”。每一层应完整获取、保留 source lineage，再按实体合并：

1. **Building**：Overture building 与 OSM building 并行；Overture 的非 OSM building source 可补足覆盖。若 `sources[]` 指向 OSM，则它与直接 OSM 对象是同一谱系，只合并为一个候选，不增加一次独立票。空间重叠、GERS/source record、名称与 building part 关系共同用于 conflation。
2. **Road/circulation**：Overture transportation 作为规范化中心线、connectivity 与 width-rules 主层；直接 OSM 作为原始 tags、area-highway/plaza 和较新变更补充。按 OSM record/source lineage 合并，不因两边都出现而提高独立置信度。
3. **Water**：Overture `base/water` 作为规范化点/线/面主层；OSM 原始对象补充 source tags 与更新检查。Overture 官方说明其 water release 来源是 OSM，所以二者默认同源，不是交叉验证。Source: [Overture Water](https://docs.overturemaps.org/schema/reference/base/water/).
4. **Vegetation**：OSM `natural/landuse` 与 Overture `base/land`/`land_use` 合并为明确对象层；Overture `land_cover`（ESA WorldCover）仅在明确对象缺失处给出粗覆盖候选。tree/tree-row 只使用明确点/线，不从 coarse land cover 猜测。
5. **Sports**：OSM pitch/track 精确对象优先；Overture `land_use` 作为受控、版本化的规范化镜像/补充。stadium、sports_centre、recreation_ground 只参与容器和缺口判断，不自动变成可生成场地。

每个合并后候选至少保存：provider、release/snapshot、source dataset、source record ID/version/update time、license/attribution、原始 geometry type、完整 MultiPolygon/holes、原始 tags/classes、坐标系（OSM/Overture 为 WGS-84）、转换和裁剪记录。Overture 的 `sources[]` 正是为属性来源信息设计，不能在桥接为简化 GeoJSON 时丢弃。Source: [Overture Sources](https://docs.overturemaps.org/schema/reference/core/sources/).

## Confidence and completeness

OSM 官方明确说明完整度随地区和地物类型显著变化，一所学校可能精细到门、路径和房间，附近学校也可能只有一个点；“没有查询到”不能推导为“现实中不存在”。Sources: [OSM Completeness](https://wiki.openstreetmap.org/wiki/Completeness), [OSM Limitations](https://wiki.openstreetmap.org/wiki/Limitations).

推荐置信度规则：

- **High**：语义 tag/class 精确；几何类型适合该地物；relation 完整有效；在校园边界内；无相互矛盾的更新证据；宽度/边界不是默认猜测。
- **Medium**：对象存在且几何有效，但宽度由规则推断、只有粗 land cover、缺少 sport subtype，或来源同源但表达存在差异。
- **Low / known gap**：point-only 无法形成所需 area、relation 不完整、几何自交、只有容器而无实际场地、默认宽度影响显著，或结构化图层为空但不能证明不存在。

来源名称或对象是否有 `name=*` 不能单独决定几何置信度。置信度只决定审核队列，不自动确认候选。每层还应输出独立 coverage report：查询范围、tile 数、分页/行组是否穷尽、异常、截断、有效/无效/待审核数量和已知缺口。

公共 Overpass 是共享服务，官方页面列出配额与实例差异；V1.1 已决定使用受控服务，因此不应把公共实例作为唯一生产后端。受控服务可使用自管 OSM extract/Overpass 数据库，并以固定 snapshot 生成完整、可重放的 layer result。Sources: [Overpass API instances and usage policy](https://wiki.openstreetmap.org/wiki/Overpass_API), [Overpass QL](https://wiki.openstreetmap.org/wiki/Overpass_API/Overpass_QL).

## Licensing and source independence

OSM 是 ODbL 数据，产品必须显示 attribution，并在分发数据库或衍生数据库时遵守相应义务。Overture 的 Base、Buildings、Transportation 主题也标为 ODbL，并要求保留上游 attribution；Overture Base 多数来自 OSM，Transportation 来自 OSM、TomTom 等，Buildings 则还可能来自 Microsoft、Google Open Buildings、Esri 等。Sources: [OSM copyright and licence](https://www.openstreetmap.org/copyright), [Overture attribution and licensing](https://docs.overturemaps.org/attribution/), [Overture Buildings Guide](https://docs.overturemaps.org/guides/buildings/), [Overture Transportation Guide](https://docs.overturemaps.org/guides/transportation/).

因此“OSM 与 Overture 都命中”不天然代表两个独立来源。必须检查 Overture feature 的 `sources[]`：同一个 OSM record 只算一次；只有明确的非 OSM dataset 才可作为额外来源。Overture `land_cover` 的 ESA WorldCover 谱系与 OSM 明确对象不同，可以作为独立的粗覆盖提示，但粒度不同，不能用“像素覆盖一致”自动确认精确边界。

## Gaps in the current repository

1. **受控服务只提供建筑**：`services/README.md:3-13` 和 `services/overture_bridge.py:353-363` 只有 Overture building endpoint；没有 transportation、water、land、land_use、land_cover，也没有受控 OSM endpoint。
2. **桌面端仍直连公共 Overpass**：`native/crates/campus-services/src/lib.rs:550-590` 使用 `overpass-api.de` 与 `overpass.kumi.systems`，与“使用受控服务”的新决定不一致。
3. **查询 vocab 不完整**：`lib.rs:550-567` 仅查 building、所有 highway、部分 water、少量 landuse/leisure；缺少 node tree、tree-row/wood/scrub、area:highway，以及 water/vegetation 的 multipolygon relations 等。
4. **relation 被错误压扁**：`OverpassMember` 在 `lib.rs:531-535` 不保留 member type/ref/role；`lib.rs:741-748` 只选最长 member geometry，无法组装多外环与 inner holes。
5. **Overture 多面也被压扁**：`lib.rs:695-722` 的 `largest_geojson_ring` 只保留最大 ring，丢弃 MultiPolygon 其他部分和内洞。
6. **分类会误判**：`lib.rs:823-843` 把未命中特定 sports 值的任意 `landuse` 或 `leisure` 都归为 Vegetation；例如查询中的 `landuse=recreation_ground` 会被误分到 Vegetation，而 `leisure=park` 也可能被误当成完整植被面。
7. **领域模型无法表达真实几何**：`campus-state/src/lib.rs:556-575` 的 `MapCandidate` 只有一条 `points` 数组，没有 Point/LineString/Polygon/MultiPolygon 类型、holes、多个 parts、road subtype、width rules 或 sports nesting。
8. **置信度与完整性不足**：`lib.rs:769-774` 只根据是否有 name 将 OSM 候选定为 High/Medium；`source_snapshot_id` 仍为 `None`。客户端请求 Overture `limit=500`（`lib.rs:627`），服务却硬截到 `MAX_LIMIT=200`（`overture_bridge.py:33,360`），且没有分页完成证明。
9. **release 默认漂移**：`overture_bridge.py:361` 默认 `latest`；V1.1 的已保存项目和审核结果需要固定 release/snapshot，后续更新只能成为新候选，不能静默替换已确认几何。

这些 gap 表明当前 UI 显示了五类候选，不等于五类数据已经被完整、正确获取。V1.1 至少需要一个按 Campus Boundary、feature kind 和 pinned snapshot 查询的受控 acquisition contract，以及能够表达真实几何与 provenance 的新 candidate schema。

## What structured data still cannot solve

即使完整接入 OSM/Overture，以下问题仍可能存在：

- 新建或近期改造但尚未进入开放数据的校园道路、球场、水面和绿化；
- 只有中心线、缺少真实宽度或岸线的道路与河流；
- 被树冠/建筑遮挡的窄路，或树冠、草地、灌木之间精细边界；
- 只有 sports-centre/stadium 容器、没有单个 pitch/track 的情况；
- sport subtype、surface、lanes、tree spacing 等生成需要但 tags 缺失的属性；
- 数据相互冲突或已经过时。

这些才是视觉/其他证据的合理作用域。若 V1.1 继续视觉补缺，输入必须来自**明确允许分析和生成衍生几何**的影像/栅格或当地开放数据；不能使用未获许可的高德截图。视觉候选必须保留影像日期、分辨率、许可、模型/算法版本和置信度，并且只填充结构化 coverage report 已识别的缺口，不覆盖更强的结构化几何。

若找不到许可清晰且能覆盖 Putuo Campus 的影像，V1.1 仍可依靠完整结构化五层完成主流程，并把上述残差保存为 `known feature gap`，允许带明确警告继续；不能用不合规截图或猜测几何掩盖缺口。

## Recommended V1.1 decision

1. 将“受控 OSM/Overture 服务”定义为五层数据服务，不再等同于 Overture building bridge。
2. 在确认并保存 Campus Boundary 后，按 boundary 全覆盖查询五类数据；每层独立分页/分块、组装关系、裁剪、去重并生成 coverage report。
3. 道路、水体、植被和体育设施并不必须依赖视觉识别：OSM 与 Overture 已有相应的结构化 schema。视觉识别只处理结构化数据明确留下的缺口。
4. OSM/Overture 同源记录合并为一个 evidence lineage；不能用同一 OSM 对象在两个分发渠道出现来虚增置信度。
5. 把完整候选几何、source snapshot、审核决定、known gaps 与 Campus Boundary 一起写入 Campus Reconstruction Project。再次打开项目时直接恢复并跳到下一未完成部分，不重新查询或要求重新审核；用户主动“检查数据更新”时才产生新版候选。
