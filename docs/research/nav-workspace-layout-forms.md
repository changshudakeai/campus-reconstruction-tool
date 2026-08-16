# 地图主区 + 侧边抽屉的布局形态调研：官方设计规范与真实应用实例

> **调研日期**：2026-08-15
> **背景**：五步工作区已按 T34 采用"顶部五步条 + 地图主画面 + 左侧可收拉抽屉
> （做法 A：抽屉展开地图右移让位）"，评审步按 T38/ADR-0016 同为抽屉样式。本报告
> 回答"这种布局形态是否符合主流官方规范、真实地图/GIS/创作类应用怎么做、哪些
> 细节值得并入后续工单"，为负责人层面的布局讨论与工单细化提供一手依据。
>
> **约束（来自项目 ADR）**：ADR-0027 主窗口为顶部横向步骤条（五步，当前格高亮、
> 已完成打勾），主区域整屏显示当前步骤；ADR-0005 界面文字全部外置；ADR-0037 S1
> 只呈现与转发；ADR-0041 边界是唯一必填项；产品基线确认五步入口不是强制关卡。
>
> **调研方法**：仅引用一手来源——各平台官方设计规范文档（Microsoft Learn、
> Apple HIG、Material Design 3 / Android 开发者文档、IBM Carbon Design System）
> 与官方产品文档（Esri ArcGIS Pro、QGIS、Figma、Blender）。Apple HIG 与
> Material 站点为纯前端渲染页面，正文经官方 JSON 数据端点 / Wayback 快照核对；
> Google Maps 与高德网页版无公开布局规范文档，仅作"实况观察"引用并在文中标注。

---

## 一、结论速览

**推荐保持并细化当前布局形态：顶部步骤条（导航）+ 地图主区（内容）+ 左侧抽屉
（辅助面板），抽屉展开时地图让位（做法 A，推挤式），收起时地图恢复全宽。**

一条理由：微软 Fluent 明确推荐"≤5 个同等重要的顶级入口用顶部导航"，苹果 HIG
建议侧边面板放在视图前缘（leading side），Material 3 把抽屉定位为大尺寸设备
的导航形态，而 ArcGIS Pro / QGIS / Figma / Blender 等"画布/地图优先"专业应用
全部采用"顶部命令区 + 中央画布 + 侧边面板"的同一骨架。本项目当前形态与行业
主流一致，不需要推倒重来。

需要并入工单的具体细化（详见第四节）：

1. **做法 A（让位）是主流程的正确选择**：Material 3 把"常驻、影响布局网格"的
   抽屉（permanent）与"遮罩、阻断其余内容"的抽屉（modal）明确区分；圈边界、
   定朝向时需要"看着地图点按钮"，推挤式保证地图与抽屉同屏可见，遮罩式会挡住
   地图。做法 B 只应作为窄窗口下的自适应回退。
2. **窄窗口自适应**：Fluent 规范给出 640/1008 px 两档断点；建议 ≤800px 宽时
   抽屉自动收成窄栏或遮罩模式，≥1000px 时保持展开，避免"地图被挤没"。
3. **抽屉内容不超过两级层级**：苹果 HIG 与 Carbon 一致要求侧栏最多两级、第三级
   改用页内标签；抽屉内"步骤块 + 操作"最多一层分组，不要再做嵌套菜单。
4. **关键操作不放抽屉底部**：苹果 HIG 明确提醒侧栏底部易被遮挡；确认边界/开始
   采集/导出等主操作应固定在抽屉上部或单独的操作栏，列表滚动不影响其可达
   （与 T33 的教训一致）。
5. **评审步可以更"内容优先"**：Material 3 的 standard（dismissible）抽屉正是指
   向"以内容为主、不常切换目的地"的场景（照片图库式）；评审步候选多、地图是
   主角，可让评审抽屉更宽（320 逻辑像素左右）且默认收起，符合苹果"允许隐藏
   面板、窗口变窄自动收起"的建议。

---

## 二、官方设计规范要点（一手来源）

### 1. 微软 Fluent（Windows 应用）——NavigationView

来源：[NavigationView - Windows apps（Microsoft Learn）](https://learn.microsoft.com/en-us/windows/apps/design/controls/navigationview)

Fluent 的 NavigationView 是 Windows 桌面应用的顶层导航控件，明确支持顶部与左侧
两种导航栏，并给出选择条件：

- **顶部导航（Top）**："当你有 5 个或更少、且同等重要的顶级导航类别，并且希望
  全部显示在屏幕上、给内容留更多空间时，推荐使用顶部导航。"
- **左侧导航（Left）**："当你有 5–10 个同等重要的顶级类别、希望导航非常突出时"
  使用，代价是内容空间变小。
- **LeftCompact**：只显示图标，打开时面板**覆盖**在内容之上。
- **LeftMinimal**：只显示菜单按钮，打开时面板覆盖在左侧内容之上。
- **Auto（自适应）**：默认按窗口宽度自动切换——≥1008px 展开左侧面板；641–1007px
  收成图标栏（LeftCompact）；≤640px 只留菜单按钮（LeftMinimal）。

来源：[Navigation design basics（Microsoft Learn）](https://learn.microsoft.com/en-us/windows/apps/design/basics/navigation-basics)

- 导航三原则：一致性（用标准控件与常规位置）、简洁性（导航项越少越省心，重要
  入口给足可达性，次要入口藏起来）、清晰性（路径清楚、目标明确标注）。
- 结构只有两类：扁平（lateral，页面同级互跳）与层级（hierarchical）。扁平结构
  适合"任意顺序可看、彼此独立、一组少于 8 页"的情况。
- 避免超过两级的深导航；超过两级要给面包屑。避免"跳跳糖式"折返（pogo-sticking：
  看相关内容要先回上一级再下来）。

**对本项目的含义**：五步入口恰好是"5 个同等重要、希望全部可见"的顶级导航 →
顶部步骤条正是 Fluent 推荐的形态；步骤条之下再做左侧抽屉是"页面内辅助面板"，
与 Windows 桌面软件的常规结构一致。800×666 / 1000×666 两种验收窗口宽落在
Fluent 的"中等/大"区间，抽屉常驻展开（做法 A）在其推荐范围内，窄窗口再回退
图标栏/遮罩。

### 2. Apple HIG——Sidebars（侧边栏）与 Split Views（分栏视图）

来源：[Sidebars | Apple Developer Documentation](https://developer.apple.com/design/human-interface-guidelines/sidebars)（正文经 Apple 官方 JSON 端点核对）

- 侧边栏"出现在视图的前缘（leading side），让人在 App 的区域或顶级内容集合之间
  导航"，适合文件夹、播放列表这类场景。
- "侧边栏需要大量垂直与水平空间；空间有限、或想把更多屏幕让给内容时，标签栏等
  更紧凑的控件可能更好。"
- 侧边栏可以**浮在内容之上**（Liquid Glass 层），允许把丰富内容延伸到侧栏下方。
- 最佳实践：
  - **允许用户隐藏侧边栏**（平台已知手势/按钮），但**默认不要隐藏**，保证可发现性；
  - 侧边栏层级**最多两级**，更深的数据层级应改用分栏视图（中间加内容列表）；
  - 分组层级多时用展开控件（disclosure），控制纵向占用；
  - macOS：**窗口变窄时自动隐藏/展开侧边栏**（邮件 App 缩小窗口会收起侧栏给正文
    让位）；**不要把关键信息或操作放在侧边栏底部**（用户移动窗口时常遮住底边）。

来源：[Split views | Apple Developer Documentation](https://developer.apple.com/design/human-interface-guidelines/split-views)（正文经 Apple 官方 JSON 端点核对）

- 分栏视图用于"同时显示多层内容层级并支持在其间导航"，典型是前导栏 + 内容列表 +
  详情。
- 官方示例即画布类应用：**Keynote 用分栏面板围绕主幻灯片画布**——左侧导航器、
  备注与检查器面板分别停在画布四周。
- 最佳实践：
  - 每个通向详情的栏里**持续高亮当前选中项**，帮助用户保持方位感；
  - **允许隐藏面板**（编辑场景下减少干扰、给画布让位），并提供多种恢复方式
    （工具栏按钮 + 菜单命令 + 快捷键）；
  - 面板可拖拽调宽时，设置合理的**最小/最大宽度**，保持分隔条可见；
  - 默认分栏可让主栏占 1/3、次栏占 2/3，也可对半。

**对本项目的含义**：左侧抽屉（leading side）、顶部步骤条高亮当前步、地图始终
为"详情/主内容"——这些正是 HIG 的分栏/侧栏形态。两个可直接落地的点：① 主操作
不要沉底；② 抽屉默认展开、但允许收起，且窗口变窄时自动收起。

### 3. Material Design 3 / Android——抽屉的三种语义

来源：[Navigation drawer – Material Design 3](https://m3.material.io/components/navigation-drawer/overview)：
"导航抽屉让用户在大尺寸设备上切换 UI 视图，并提供对 App 内目的地的访问。"（正文
为前端渲染，此句取自官方页面描述并经 Wayback 快照核对）

Material 3 组件索引把导航形态按设备宽度分工：小尺寸用导航栏（bottom bar）、中
尺寸用导航轨道（rail）、大尺寸用导航抽屉。Android 官方 Compose 文档对三种抽屉的
语义写得很明确：

- **常驻抽屉（PermanentNavigationDrawer）**："抽屉常与内容相邻，并**影响屏幕的
  布局网格**；常驻抽屉始终可见，适合**频繁切换目的地**的场景。手机屏幕改用模态
  抽屉。"（[PermanentNavigationDrawer | Android Developers](https://developer.android.com/reference/kotlin/androidx/compose/material3/PermanentNavigationDrawer.composable)）
- **模态抽屉（ModalNavigationDrawer）**："模态抽屉用**遮罩（scrim）阻断与其余
  内容的交互**；它悬浮在大部分 UI 之上，**不影响布局网格**。"（[ModalNavigationDrawer | Android Developers](https://developer.android.com/reference/kotlin/androidx/compose/material3/ModalNavigationDrawer.composable)）
- **标准可关闭抽屉（DismissibleNavigationDrawer）**："标准抽屉可用于**以内容为主
  的布局**（如照片图库），或用户不常切换目的地的 App；应提供可见的导航菜单图标
  来开合抽屉。"（[DismissibleNavigationDrawer | Android Developers](https://developer.android.com/reference/kotlin/androidx/compose/material3/DismissibleNavigationDrawer.composable)）

**对本项目的含义**：T34 的做法 A（抽屉展开、地图让位）就是 Material 的"常驻
抽屉影响布局网格"语义，适用于五步流程中"地图与抽屉需要同时可见、来回操作"的
步骤（圈边界、定朝向）；做法 B（覆盖地图）对应"模态抽屉 + 遮罩"，会阻断地图
交互，只适合临时任务或窄窗口。评审步（内容优先、不频繁切换）对应"标准可关闭
抽屉"——这正是 Material 官方给照片图库类布局的处方。

### 4. IBM Carbon——UI Shell 左面板

来源：[UI shell left panel – Carbon Design System v10](https://v10.carbondesignsystem.com/components/UI-shell-left-panel/usage/)

- UI Shell 由**顶栏（header）+ 左面板 + 右面板**组成；顶栏是最高层导航，左面板
  承担产品内的次级导航，右面板承载系统级动作/内容。
- "当次级导航项**超过五个**，或**预期用户会频繁在次级项之间切换**时，使用左面板。"
- 子菜单点击展开时**把下方其他项向下推**（推挤式，与做法 A 同语义）；折叠再次
  点击标题。
- 左面板**不支持三级导航**；第三层内容用**页内标签页**承载。

**对本项目的含义**：五步工作区里，左侧抽屉不是"导航"而是"当前步骤的操作面板"
（每步一个抽屉、内容一层），完全在 Carbon 的推荐范围内；若未来抽屉内出现
"分组→子分组"结构，应改用页内标签而不是嵌套菜单。

---

## 三、真实应用实例（官方文档 / 实况观察）

### 1. ArcGIS Pro（Esri 官方文档）

来源：[User interface comparison | ArcGIS Pro documentation](https://doc.esri.com/en/arcgis-pro/latest/get-started/user-interface.html)

"ArcGIS Pro 用**功能区标签页（ribbon tabs）与面板（panes）**访问功能，二者会
**根据当前上下文动态变化**。"界面组成：顶部 Project 标签 + Command Search +
功能区（按标签组织命令组）、**Contents 面板（左侧，列出与当前视图相关的图层/
内容）**、视图（地图/场景/表格，是主要工作区）、**Catalog 面板（右侧，管理项目
条目）**。

要点：专业 GIS 桌面软件 = 顶部功能区（上下文敏感）+ 中央地图视图 + 左右可停靠
面板。我们的"顶部步骤条（上下文敏感切换）+ 地图主区 + 左抽屉"与之同构。

### 2. QGIS（官方用户手册）

来源：[7. QGIS GUI – QGIS Documentation](https://docs.qgis.org/3.34/en/docs/user_manual/introduction/qgis_gui.html)

"QGIS 图形界面由五类组件构成：**菜单栏、工具栏、面板、地图视图、状态栏**。"
面板与工具栏可移动/停靠，地图视图为中央工作区。

### 3. Figma（官方帮助中心）

来源：[Explore the navigation bar and left sidebar – Figma Help](https://help.figma.com/hc/en-us/articles/360039831974-Explore-the-navigation-bar-and-left-sidebar)
与 [Design, prototype, and explore layer properties in the right sidebar – Figma Help](https://help.figma.com/hc/en-us/articles/360039832014-Design-prototype-and-explore-layer-properties-in-the-right-sidebar)

设计文件界面 = "工具栏 + 导航栏 + **左、右面板** + 可滚动画布"；"左面板位于导航
栏旁，内容随所选标签动态变化，**宽度可拖拽调整**"；右面板（属性）显示选中图层
的名称、布局与颜色。

要点：画布类专业工具的标配就是"窄导航条 + 内容侧栏 + 大画布"，且侧栏宽度允许
用户微调——抽屉宽 280–320 逻辑像素"执行时可微调"与此一致。

### 4. Blender（官方手册）

来源：[Window System Introduction – Blender 5.2 LTS Manual](https://docs.blender.org/manual/en/latest/interface/window_system/introduction.html)

"Blender 界面分为三大部分：**顶部 Topbar**（主菜单、保存/导入导出/设置）、
**中部 Areas**（主工作区）、**底部状态栏**（快捷键提示与统计）。"区域可拆分、
合并、改尺寸。

### 5. Google Maps 网页版 / 高德地图网页版（实况观察，无官方布局文档）

观察（访问 https://www.google.com/maps 与 https://ditu.amap.com 所见）：

- 两者均为"**地图占满主区 + 左侧搜索/结果面板**"：左侧面板显示搜索结果、路线
  与收藏，地图在其余空间铺满；面板收起时地图立即回铺全宽。
- 窄窗口下左侧面板会变为覆盖式（遮罩/浮层），而不是把地图挤到看不见。
- 顶部的搜索框/工具条常驻，负责"当前位置 + 主操作"。

这印证了消费级地图应用的行业惯例：**地图主区 + 左侧面板 + 顶部工具条**，面板
展开推挤、窄窗口回退覆盖。

### 6. Apple Keynote（经 HIG 官方文档）

苹果 HIG 分栏视图一节以 Keynote 为例：**幻灯片导航器、备注、检查器三个面板围绕
主幻灯片画布**，且允许隐藏面板给画布让位。说明画布优先 + 侧面板 + 可隐藏是
苹果官方认可并用于自家旗舰应用的形态。

---

## 四、可直接并入工单的具体建议

以下建议按"负责人可见的产品语言"表述，可直接合并进 T34 后续工单或评审步工单。

### 建议 1：主流程保留"地图让位"（做法 A），窄窗口才用覆盖

- 步骤 ①②③⑤ 的抽屉展开时地图右移让位（现状做法 A 不变），保证"看着地图点
  按钮"全程成立；抽屉收起地图恢复全宽。
- 窗口宽度较窄（建议以 800 逻辑像素为界，参考 Fluent 640/1008 两档）时，抽屉
  展开改为覆盖模式并带半透明遮罩，或自动收成窄图标栏；宽窗口（≥1000px）保持
  常驻展开。验收窗口 800×666 / 1000×666 两档正好覆盖这两种状态。

### 建议 2：抽屉内内容最多一层分组，不做嵌套菜单

- 每个步骤的抽屉 = 顶部操作区 + 列表/参数区 + 底部固定操作栏；如需给列表分类
  （如评审六类标签），用页内标签或分组标题，不出现第二级展开菜单。
- 依据：苹果 HIG"侧栏最多两级"、Carbon"左面板不支持三级，第三层用页内标签"。

### 建议 3：关键操作不放抽屉最底部，操作栏固定可达

- "确认边界 / 开始采集 / 开始导出 / 封账"等主操作固定在一处（抽屉顶部操作区或
  独立的底部固定栏），列表滚动不得把它们推出视口（延续 T33 的教训）。
- 依据：苹果 HIG"不要把关键信息或操作放在侧边栏底部"。

### 建议 4：抽屉默认展开、可收起，收起后地图全宽

- 五步主流程进入时抽屉默认展开（保持可发现性，苹果 HIG）；用户可收起，收起态
  记忆保留在会话内，切步/切方案不丢。
- 依据：苹果 HIG"允许隐藏侧栏，但默认不要隐藏"；Google Maps/高德"面板收起后
  地图回铺"。

### 建议 5：评审步（步骤④）按"内容优先"细化

- 评审抽屉建议宽 320 逻辑像素（T34 允许 280–320 微调的上沿），候选列表滚动、
  批量操作栏固定；"定位到地图"后可选自动收起抽屉，让地图框住候选（与
  T38 的 setFitView 配合）。
- 依据：Material 3"标准可关闭抽屉适合以内容为主的布局（照片图库式）"；
  T38/ADR-0016 已决策评审同用左抽屉，本建议只细化宽度与默认收起行为，不改布局
  范式。

### 建议 6：顶部步骤条保持"当前步高亮 + 已完成打勾 + 未解锁置灰"

- 依据：苹果 HIG 分栏"持续高亮当前选中项以保持方位感"；Fluent"顶部导航适合
  5 个及以下同等重要入口"；ADR-0027 已锁定该形态，无需改动。

### 建议 7：上下文（校区名/方案名）与右上角辅助入口保持常驻

- 依据：ADR-0027 子决策（已拍板）；苹果 HIG"窗口变窄自动收起侧栏"只作用于
  抽屉，不作用于顶部步骤条与右上角图标区。

### 不建议采纳的形态（避免返工）

- 五步改左侧导航栏（ADR-0027 已否决方案 B）：Fluent 左侧导航适合 5–10 项且
  需要"非常突出"，但本项目五步是线性旅程，顶部步骤条更贴 Fluent"≤5 项顶部
  导航"建议，且已验收。
- 抽屉默认覆盖地图（做法 B 为主）：Material 模态抽屉语义是"阻断其余内容"，
  与"边看地图边操作"的主流程冲突；只作为窄窗口回退。
- 抽屉内做多级嵌套菜单（Carbon/苹果均反对）。

---

## 五、来源清单

| 来源 | 内容 | 核对方式 |
|------|------|----------|
| [NavigationView – Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/design/controls/navigationview) | 顶部/左侧导航选择条件、LeftCompact/LeftMinimal 覆盖语义、640/1008 断点 | 直接读取正文 |
| [Navigation design basics – Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/design/basics/navigation-basics) | 导航三原则、扁平/层级结构、≤2 级、避免 pogo-sticking | 直接读取正文 |
| [Sidebars – Apple HIG](https://developer.apple.com/design/human-interface-guidelines/sidebars) | 前缘侧栏、浮于内容、可隐藏、≤2 级、底部忌放关键操作、窗口变窄自动收起 | Apple 官方 JSON 端点（正文为前端渲染） |
| [Split views – Apple HIG](https://developer.apple.com/design/human-interface-guidelines/split-views) | 分栏围绕主画布（Keynote）、持续高亮选中、可隐藏面板、最小/最大宽度 | Apple 官方 JSON 端点 |
| [Navigation drawer – Material Design 3](https://m3.material.io/components/navigation-drawer/overview) | 抽屉用于大尺寸设备切换视图 | 官方页面描述 + Wayback 快照 |
| [PermanentNavigationDrawer – Android Developers](https://developer.android.com/reference/kotlin/androidx/compose/material3/PermanentNavigationDrawer.composable) | 常驻抽屉影响布局网格、适合频繁切换 | 直接读取正文 |
| [ModalNavigationDrawer – Android Developers](https://developer.android.com/reference/kotlin/androidx/compose/material3/ModalNavigationDrawer.composable) | 模态抽屉遮罩阻断交互、不影响布局网格 | 直接读取正文 |
| [DismissibleNavigationDrawer – Android Developers](https://developer.android.com/reference/kotlin/androidx/compose/material3/DismissibleNavigationDrawer.composable) | 标准抽屉适合内容优先布局、需可见开合图标 | 直接读取正文 |
| [UI shell left panel – Carbon v10](https://v10.carbondesignsystem.com/components/UI-shell-left-panel/usage/) | 顶栏+左/右面板骨架、>5 项或频繁切换用左面板、子菜单推挤、不支持三级 | 直接读取正文 |
| [User interface comparison – ArcGIS Pro](https://doc.esri.com/en/arcgis-pro/latest/get-started/user-interface.html) | 功能区+面板、Contents 左面板、视图为主工作区 | 直接读取正文 |
| [QGIS GUI – QGIS Documentation](https://docs.qgis.org/3.34/en/docs/user_manual/introduction/qgis_gui.html) | 菜单栏/工具栏/面板/地图视图/状态栏 | 直接读取正文 |
| [Figma Help – 左面板](https://help.figma.com/hc/en-us/articles/360039831974-Explore-the-navigation-bar-and-left-sidebar) | 工具栏+导航栏+左右面板+画布、侧栏可调宽 | 直接读取正文 |
| [Figma Help – 右面板](https://help.figma.com/hc/en-us/articles/360039832014-Design-prototype-and-explore-layer-properties-in-the-right-sidebar) | 右侧属性面板按选中图层动态变化 | 直接读取正文 |
| [Window System – Blender Manual](https://docs.blender.org/manual/en/latest/interface/window_system/introduction.html) | Topbar + Areas + 状态栏 | 直接读取正文 |
| [Google Maps](https://www.google.com/maps) / [高德地图网页版](https://ditu.amap.com) | 地图主区+左侧面板、窄窗口覆盖回退 | 实况观察（无官方布局文档，标注为观察） |

---

## 六、与现有文档的衔接

- 本报告不改变任何已接受的 ADR；它是对 ADR-0027（顶部步骤条）、T34（地图主区 +
  左侧抽屉做法 A）、ADR-0016/T38（评审抽屉化）的**佐证与细化**。
- 若采纳"窄窗口自动收起抽屉"与"评审抽屉默认收起/更宽"两项建议，需在对应工单的
  验收标准里增加 800×666 与 1000×666 两档的抽屉状态断言（沿用 T34 的
  `workspace-map-slot-*` 上报与矩形互不相交断言）。
- 所有用户可见文案仍走 `zh-CN.json`（ADR-0005），布局形态不引入新的可见文案
  之外的变化；S1 仍只呈现与转发（ADR-0037）。
