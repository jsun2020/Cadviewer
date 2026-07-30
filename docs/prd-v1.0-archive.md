# Cadviewer 产品需求文档（PRD）

> 文档版本：1.0  
> 对应产品版本：Cadviewer v0.1.x  
> 文档状态：基线版  
> 更新日期：2026-07-29  
> 目标平台：Windows 10/11 x64  
> 产品形态：免安装便携式桌面应用 + 命令行转换工具

---

## 1. 文档目的

本文档用于定义 Cadviewer 的产品定位、当前能力、已知限制、目标用户、核心需求、质量标准与后续版本计划。

本文档特别说明当前版本的 Layout/Viewport 限制：

- 当前版本不是真正的多 Layout 查看器。
- 当前版本会从 DWG 中自动选择一个标题区域作为唯一二维场景。
- 未选中的模型空间区域会在解析阶段被裁剪。
- 图纸空间（Paper Space）实体当前被跳过。
- 当前无法通过缩放或平移找到其他 Layout。
- 当前 PDF 仅导出已加载的单一场景，不支持全部 Layout 多页导出。

后续版本必须将“单场景查看”升级为“模型空间 + 多 Layout 切换 + 多页 PDF 导出”。

---

## 2. 产品概述

### 2.1 产品名称

Cadviewer

### 2.2 一句话定位

一款面向 Windows 的极简、便携、快速启动的 DWG/DXF 二维查看器，并提供高效率的矢量 PDF 转换能力。

### 2.3 产品愿景

让不需要完整 CAD 编辑能力的用户，可以在无需安装大型商业 CAD 软件的情况下：

1. 快速打开 DWG/DXF 文件。
2. 正确查看常见二维图纸、块、图层颜色和中文文字。
3. 在模型空间和各个 Layout 之间快速切换。
4. 将当前 Layout 或全部 Layout 高效导出为 PDF。
5. 将整个程序复制到任意 Windows 电脑后直接运行。

### 2.4 核心产品原则

1. **只读优先**：不编辑、不回写 DWG，降低复杂度和文件损坏风险。
2. **打开即看**：用户不需要配置工程、插件或运行环境。
3. **布局准确**：块、图层、文字、Layout 和 Viewport 的正确性优先于花哨功能。
4. **中文可靠**：中文 UI、中文文件名、中文 TEXT/MTEXT 不得出现乱码。
5. **按需加载**：大型 DWG、多个 Layout 和嵌套块不得一次性全部展开。
6. **便携透明**：无安装、无注册表依赖，运行时组件和许可证随包提供。
7. **查看与转换一致**：屏幕中选中的 Layout 应与导出的 PDF 内容一致。

---

## 3. 背景与用户问题

### 3.1 背景

工业、建筑、暖通、电气和装修项目中经常存在以下情况：

- 一个 DWG 中包含多张模型空间图纸。
- 一个 DWG 中包含多个 Paper Space Layout。
- 每个 Layout 中存在一个或多个 Viewport。
- 图纸大量使用块、嵌套块、属性、中文文字、图层颜色和标注。
- 用户只需要查看和转 PDF，并不需要编辑能力。

传统完整 CAD 软件安装体积大、启动慢、授权成本高，不适合临时查看、生产线终端、文件归档和批量转换场景。

### 3.2 当前主要痛点

1. 轻量查看器容易丢失块和 Layout。
2. 中文文字容易因字体、代码页或 MTEXT 控制码而乱码。
3. 直接“缩放到全部实体”会把多个图纸压缩成很小的一团。
4. 大型 DWG 完全展开后，内存、节点数和转换时间急剧增加。
5. 屏幕显示与 PDF 导出结果经常不一致。
6. 用户无法明确知道当前看到的是模型空间、Layout，还是自动裁剪区域。

---

## 4. 目标与非目标

### 4.1 产品目标

#### G1：可靠查看常见二维 DWG/DXF

- 正确显示线、圆弧、多段线、文字、块、颜色等常见元素。
- 中文标题、房间名称、备注、设备名称和表格文字可读。
- 支持缩放、平移和适合窗口。

#### G2：支持真正的多 Layout 工作流

- 识别模型空间和所有 Layout。
- 显示 Layout 名称与顺序。
- 支持 Layout 切换。
- 正确处理 Layout 中的 Viewport。
- 支持当前 Layout 和全部 Layout 导出。

#### G3：保持便携和低使用门槛

- 解压即用。
- 无需安装 LibreDWG、Rust、字体插件或其他运行时。
- 不要求管理员权限。

#### G4：保持较高的打开和转换效率

- 首屏优先显示。
- Layout 按需解析。
- 已访问 Layout 缓存。
- 后台转换，不阻塞 UI。

#### G5：形成可持续验证体系

- 建立多 Layout、中文、块、颜色、标注和 PDF 的回归样本。
- 每次版本发布前执行自动测试和视觉对比。

### 4.2 非目标

以下能力不属于近期范围：

- 编辑、删除或创建 CAD 实体。
- 保存或覆盖 DWG。
- 完整替代 AutoCAD、浩辰 CAD、天正等专业软件。
- 三维建模、三维渲染、材质和光照。
- CAD 协同审图、批注云线、版本合并。
- 完整打印驱动、CTB/STB 打印样式编辑。
- 完整支持所有第三方自定义对象和代理对象。

---

## 5. 目标用户

### 5.1 一线查看用户

典型角色：

- 生产车间人员
- 施工现场人员
- 设备安装人员
- 物业与运维人员

核心需求：

- 快速找到目标 Layout。
- 放大查看局部细节。
- 中文标注可读。
- 不误操作修改原文件。

### 5.2 设计与项目管理人员

典型角色：

- 建筑、暖通、电气、装修设计人员
- 项目经理
- 审图与归档人员

核心需求：

- 检查 DWG 中包含哪些 Layout。
- 快速切换布局。
- 导出当前布局或全部布局 PDF。
- PDF 页名、顺序和 Layout 一致。

### 5.3 批量转换与系统集成人员

典型角色：

- 文档归档系统维护人员
- 自动化脚本开发人员
- 批量资料处理人员

核心需求：

- 使用命令行转换。
- 获得明确的退出码与错误信息。
- 可指定 Layout。
- 可批量导出全部 Layout。
- 输出稳定、可重复。

---

## 6. 核心使用场景

### 场景 A：打开单图 DWG

1. 用户双击 `Cadviewer.exe`。
2. 点击“打开”或拖入 DWG。
3. 应用显示模型空间或唯一 Layout。
4. 用户滚轮缩放、拖动平移。
5. 用户导出当前视图对应的 PDF。

### 场景 B：打开包含多个 Layout 的 DWG

1. 用户打开 DWG。
2. 应用先完成 Layout 索引。
3. 左侧或顶部显示“模型”和所有 Layout 名称。
4. 应用打开 DWG 保存时的活动 Layout；无法确认时打开第一个可见 Layout。
5. 用户点击其他 Layout。
6. 应用按需解析并显示对应页面。

### 场景 C：导出当前 Layout

1. 用户切换到目标 Layout。
2. 点击“导出 PDF”。
3. 选择“当前 Layout”。
4. 输出单页 PDF。
5. PDF 页面大小、方向、裁剪范围与该 Layout 一致。

### 场景 D：导出全部 Layout

1. 用户点击“导出 PDF”。
2. 选择“全部 Layout”。
3. 应用按 Layout 顺序逐页转换。
4. 输出一个多页 PDF。
5. 进度区显示当前正在转换的 Layout。
6. 转换失败时明确指出失败的 Layout，并提供继续或终止策略。

### 场景 E：命令行批量转换

```powershell
Cadconvert.exe input.dwg output.pdf
Cadconvert.exe input.dwg output.pdf --layout "Layout1"
Cadconvert.exe input.dwg output.pdf --all-layouts
Cadconvert.exe input.dwg output-directory --split-layouts
```

其中：

- 默认行为必须明确且稳定。
- `--layout` 导出指定 Layout。
- `--all-layouts` 输出多页 PDF。
- `--split-layouts` 每个 Layout 输出独立 PDF。

---

## 7. 当前版本能力基线（v0.1.x）

### 7.1 已实现功能

- 打开 `.dwg` 和 `.dxf`。
- 拖放打开文件。
- `Ctrl+O` 打开文件。
- 鼠标滚轮缩放。
- 鼠标拖动平移。
- 双击或点击按钮适合窗口。
- 导出单页矢量 PDF。
- 独立命令行工具：

```powershell
Cadconvert.exe input.dwg output.pdf
```

- GUI 命令行转换：

```powershell
Cadviewer.exe --convert input.dwg output.pdf
```

- 便携运行，无安装程序。
- 使用 Windows 中文字体显示 UI 和图纸中文。
- 深色 CAD 查看背景。
- 支持图层颜色和实体颜色。
- 支持块和嵌套块展开。
- 后台加载和后台 PDF 导出。
- 视口栅格尺寸上限为 4096 像素，避免无限放大造成显存压力。

### 7.2 当前主要支持的二维实体

- LINE
- CIRCLE
- ARC
- ELLIPSE
- LWPOLYLINE（包含 bulge 圆弧）
- POLYLINE
- SPLINE（当前为近似显示）
- TEXT
- MTEXT
- ATTRIB
- ATTDEF
- POINT
- SOLID
- TRACE
- 3DFACE（二维投影）
- LEADER
- MLINE
- INSERT
- MINSERT
- 常见 DIMENSION 匿名块

### 7.3 当前未完整支持的内容

- 真正的多 Layout 切换
- Paper Space 完整渲染
- Layout Viewport 变换与裁剪
- Viewport 独立冻结图层
- HATCH
- IMAGE/OLE
- 外部参照 XREF 的完整解析
- SHX 形文件与全部 SHX 字体
- 复杂 DIMENSION 语义重建
- 复杂 SPLINE/NURBS 精确曲线
- 三维实体、ACIS、代理对象
- CTB/STB 打印样式
- 多页 PDF

---

## 8. 当前 Layout/Viewport 限制

本节是当前版本最重要的已知限制说明。

### 8.1 当前实际行为

当前加载流程为：

1. LibreDWG 将 DWG 转换为完整 DXF。
2. Cadviewer 读取 DXF 的 TABLES、BLOCKS 和 ENTITIES。
3. 程序读取 `*ACTIVE` VPORT，作为可能的初始范围。
4. 程序扫描模型空间 MTEXT 标题，并计算一个“优选标题区域”。
5. 当找到优选标题时，该标题区域会覆盖 `*ACTIVE` VPORT。
6. 程序以该区域作为 `initial_view`。
7. 块和实体在解析阶段按 `initial_view` 裁剪。
8. 根实体中 DXF 组码 `67 != 0` 的 Paper Space 实体被跳过。
9. 最终只生成一个 SVG 场景和一个 PDF 页面。

### 8.2 为什么当前只能看到一个 Layout/图纸区域

当前看到的内容并不是一个完整 Layout 集合，而是程序自动选中的单一场景。

其他内容存在两种情况：

1. **其他模型空间图纸区域**  
   在解析阶段被视口裁剪，没有进入最终场景。

2. **Paper Space Layout 内容**  
   因组码 `67 != 0` 被跳过，没有进入最终场景。

因此：

- 其他 Layout 不是简单地位于画面外。
- 继续缩小、放大或平移无法找到其他 Layout。
- “适合窗口”只能适合当前已加载的场景。
- 当前 PDF 也只能导出这个场景。

### 8.3 当前限制产生的原因

早期版本将所有实体和块全部展开，导致：

- 多张图纸被缩放成很小的一团。
- 复杂嵌套块可能生成接近或超过一百万个图元。
- SVG/PDF 节点数量过多。
- 打开和转换耗时过长。
- 极远离主体的实体会破坏整体边界。

为优先解决“目标图正确打开”和“中文可读”，v0.1.x 采用了单视口裁剪方案。该方案解决了目标图显示问题，但牺牲了多 Layout 浏览能力。

### 8.4 当前产品风险

- UI 未明确提示“当前为自动选定区域”。
- 用户容易误认为 DWG 中只有一张图。
- PDF 导出可能遗漏未加载 Layout。
- 标题启发式选择不适用于所有专业和所有命名方式。
- 相同标题或无标题图纸可能选错区域。
- 当前行为不应作为 v1.0 的正式 Layout 方案。

### 8.5 v0.1.x 临时要求

在多 Layout 功能发布之前，应至少做到：

- 状态栏显示“当前仅加载自动选定图纸区域”。
- 检测到多个 Layout 时显示明确提示。
- 导出 PDF 前提示“仅导出当前已加载区域”。
- README 和 PRD 均记录此限制。
- 不得宣称“支持全部 Layout”。

---

## 9. 下一版本核心需求：多 Layout 支持

### 9.1 Layout 发现

系统必须解析：

- Model Space
- LAYOUT 对象
- BLOCK_RECORD 与 Layout 的关联
- `*Paper_Space`
- `*Paper_Space0`
- 其他 Paper Space 块
- Layout 名称
- Layout 顺序
- Layout 是否为当前活动布局
- 页面宽高和方向
- Layout 中的 VIEWPORT

#### 验收标准

- 对包含 1 个模型空间和 N 个 Layout 的文件，列表中显示 `模型 + N 个 Layout`。
- Layout 名称保持原始中文。
- Layout 顺序与主流 CAD 软件一致。
- 隐藏、删除或无效 Layout 不显示为正常可选项。

### 9.2 Layout 导航 UI

推荐 v0.2 采用顶部标签栏：

```text
┌─────────────────────────────────────────────────────────────┐
│ 打开  导出 PDF  − 100% +  适合窗口                         │
├─────────────────────────────────────────────────────────────┤
│ [模型] [Layout1] [五层暖通] [五层配电] [屋面]  [更多 ▾]    │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│                         图纸画布                            │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

交互要求：

- 当前 Layout 高亮。
- Layout 数量过多时支持横向滚动或“更多”菜单。
- 切换 Layout 后保留各自的缩放与中心位置。
- 首次进入 Layout 自动适合窗口。
- 已加载 Layout 再次进入时优先使用缓存。
- 切换过程中显示加载状态，不冻结窗口。

### 9.3 Model Space

模型空间需要提供两种查看模式：

1. **保存视图模式**  
   默认使用 DWG 保存的活动 View/VPORT。

2. **全部范围模式**  
   用户主动选择“显示全部模型空间”，允许查看所有并排图纸。

对于包含大量分图的模型空间，后续可增加：

- 图纸区域自动检测。
- 缩略图导航。
- 按标题区域生成“模型视图书签”。

标题启发式只能作为模型空间辅助导航，不能再代替 Layout。

### 9.4 Paper Space 渲染

Paper Space Layout 必须渲染：

- 图框
- 标题栏
- Paper Space 文字和表格
- Paper Space 块
- Paper Space 标注
- 一个或多个 VIEWPORT

Layout 页面边界必须作为最终画布边界，不得使用全局模型实体范围替代。

### 9.5 Viewport 渲染

每个 VIEWPORT 至少需要支持：

- Viewport 位置
- Viewport 宽高
- 模型空间视图中心
- 视图高度与比例
- 旋转/扭转角
- 矩形裁剪
- Viewport 开启/关闭状态
- Viewport 内模型实体变换
- Viewport 内图层冻结覆盖

后续增强：

- 非矩形 Viewport 裁剪
- 多边形裁剪
- 重叠 Viewport
- Viewport 显示顺序
- 透视与三维视图降级显示

#### 验收标准

- 同一模型内容在不同 Viewport 比例下显示正确。
- Viewport 外的模型内容不泄漏到 Layout 页面。
- Layout 内多个 Viewport 可同时显示。
- Viewport 冻结图层不出现在该 Viewport 中。
- Paper Space 文字不随模型 Viewport 比例缩放。

### 9.6 Layout 按需加载

不得在打开文件时同步展开全部 Layout。

建议流程：

1. 快速解析 Layout 索引和基础元数据。
2. 加载默认 Layout。
3. 用户切换时加载目标 Layout。
4. 后台预取相邻 Layout。
5. 使用 LRU 缓存保存最近访问场景。
6. 达到内存阈值时释放最旧场景。

建议缓存键：

```text
Document ID + Layout Handle + Render Options + Font Mapping Version
```

### 9.7 Layout 加载取消

当用户快速切换 Layout 时：

- 旧 Layout 的解析任务必须可以取消或丢弃结果。
- 只允许最新选择的 Layout 更新 UI。
- 不得出现旧 Layout 后完成并覆盖新 Layout 的情况。

---

## 10. PDF 导出需求

### 10.1 导出当前 Layout

- 默认导出当前选中的 Model/Layout。
- Layout 页面尺寸来自 DWG 页面设置或 Layout 边界。
- 输出单页 PDF。
- 屏幕和 PDF 使用同一场景数据。
- 不得因屏幕缩放和平移改变 PDF 页面内容。

### 10.2 导出全部 Layout

- 输出一个多页 PDF。
- Layout 顺序与 UI 列表一致。
- 默认不导出 Model Space，可由选项控制。
- 每页允许拥有不同纸张尺寸和方向。
- 进度显示：`正在导出 3/8：五层暖通`。
- 导出完成后显示成功页数、跳过页数和警告。

### 10.3 分 Layout 导出

允许将每个 Layout 输出为独立文件：

```text
原文件名 - Layout1.pdf
原文件名 - 五层暖通.pdf
原文件名 - 五层配电.pdf
```

文件名非法字符需要替换。

### 10.4 PDF 背景策略

提供以下模式：

- 白底打印模式（默认 PDF）
- 深色屏幕模式
- 使用 Layout 页面背景

线条颜色需要根据背景策略转换，避免白线导出到白底后不可见。

### 10.5 字体策略

优先级：

1. DWG/DXF 指定的可用字体。
2. 项目字体映射。
3. Windows 常用中文字体。
4. 用户配置的替代字体。
5. 内置回退字体。

PDF 必须保证中文可读。后续应评估字体嵌入或文字转路径，以提高跨设备一致性。

### 10.6 命令行需求

规划参数：

```text
Cadconvert.exe <input> <output>
  --layout <name>
  --all-layouts
  --include-model
  --split-layouts
  --background white|dark|layout
  --font-map <file>
  --overwrite
  --json-status
```

退出码建议：

- `0`：成功
- `1`：输入文件或解码失败
- `2`：参数错误
- `3`：指定 Layout 不存在
- `4`：部分 Layout 转换失败
- `5`：输出写入失败

---

## 11. 中文与字体需求

### 11.1 中文编码

- LibreDWG 输出的 UTF-8 字符串必须保持 UTF-8。
- 支持 ANSI_936/GBK 来源 DWG 的中文转换结果。
- 不得使用系统默认 ANSI 编码读取 DXF。
- 中文文件名、Layout 名称和导出文件名必须正常显示。

### 11.2 TEXT/MTEXT

至少支持：

- `\P` 换行
- `\~` 不换行空格
- `\U+XXXX` Unicode
- `%%d` 度数
- `%%p` 正负号
- `%%c` 直径符号
- `\S...;` 堆叠分数的可读降级
- 常见颜色、字体、宽度、高度、倾斜控制码的剥离或解析
- MTEXT 大括号分组

### 11.3 SHX

后续版本需要：

- 检测缺失 SHX。
- 显示缺失字体列表。
- 支持 SHX 到 TrueType 替换表。
- 保存用户字体映射。
- 区分普通 SHX 与大字体。

---

## 12. 图层与颜色需求

### 12.1 图层

- 解析图层名称。
- 解析开关、冻结和锁定状态。
- 不显示关闭或冻结图层。
- Layout Viewport 支持独立冻结图层。

### 12.2 颜色

- 支持 ACI 颜色。
- 支持 True Color。
- 支持 ByLayer。
- 支持 ByBlock。
- Layer 0 在块内继承插入颜色。

### 12.3 后续图层面板

v0.4 可增加只读图层面板：

- 搜索图层。
- 临时显示/隐藏。
- 恢复 DWG 默认状态。
- 图层操作仅影响当前会话，不写回 DWG。

---

## 13. 查看器交互需求

### 13.1 基本操作

- 滚轮：以鼠标位置为中心缩放。
- 左键或中键拖动：平移。
- 双击：适合当前 Layout。
- `+`/`-`：缩放。
- `Ctrl+O`：打开。
- `Ctrl+E`：导出。
- `Home`：适合当前 Layout。

### 13.2 状态显示

状态栏至少显示：

- 文件名
- 当前空间：模型或 Layout 名
- 加载状态
- 实体数量
- 当前缩放比例
- LibreDWG 警告
- 缺失字体数量
- 当前 Layout 是否为近似显示

### 13.3 错误提示

错误提示必须使用可理解的中文，不显示内部 panic 或无上下文错误码。

错误信息示例：

- “无法读取 DWG：文件可能损坏或版本暂不支持。”
- “Layout ‘五层暖通’ 中的 2 个代理对象未显示。”
- “缺少字体 hztxt.shx，已替换为宋体。”
- “仅导出了 7/8 个 Layout，Layout ‘屋面’ 转换失败。”

---

## 14. 非功能需求

### 14.1 性能目标

以下目标以发布版、Windows x64、SSD、16 GB 内存的常规办公电脑为基准。

#### 文件打开

- 1 MB 以下简单 DWG：首个 Layout 可交互时间 ≤ 3 秒。
- 10 MB 以下常规二维 DWG：首个 Layout 可交互时间 P50 ≤ 8 秒，P95 ≤ 15 秒。
- Layout 索引完成时间应显著短于全部 Layout 渲染时间。
- 已缓存 Layout 切换时间 P50 ≤ 0.5 秒，P95 ≤ 1.5 秒。

#### 交互

- 平移和缩放输入响应 ≤ 100 ms。
- 拖动过程中允许降低渲染质量，但松开后 300 ms 内启动清晰重绘。
- UI 主线程不得执行 DWG 解码、DXF 全量解析或 PDF 生成。

#### 内存

- 默认缓存内存上限建议为 1 GB。
- 单文件峰值内存目标不超过输入文件大小的 100 倍，且应设硬上限。
- 达到内存上限时释放旧 Layout，而不是崩溃。

### 14.2 稳定性

- 损坏文件不得导致应用崩溃。
- 递归块必须有深度限制。
- 图元数量、路径长度、图片尺寸必须有上限。
- 解析任务必须处理取消与超时。
- PDF 写入应先输出临时文件，成功后再替换目标文件。

### 14.3 兼容性

- Windows 10 22H2 x64
- Windows 11 x64
- 125%、150%、200% DPI
- 中文 Windows 用户名和路径
- 空格、括号及中文文件名
- 无管理员权限账户

### 14.4 便携性

- 不写注册表。
- 不强制写入 AppData。
- 默认不联网。
- 不依赖预装 LibreDWG。
- 可选设置保存在程序目录或用户指定目录。

### 14.5 安全

- 所有输入按不可信文件处理。
- 限制递归深度、数组数量、字符串长度和图元数量。
- 外部参照不得自动访问网络路径。
- 不执行 DWG 中的宏、脚本、OLE 或外部命令。

---

## 15. 建议技术架构

### 15.1 当前链路

```text
DWG
  ↓ LibreDWG dwg2dxf
完整 DXF
  ↓ Cadviewer 自定义解析器
单一 Scene
  ├─ resvg → 屏幕纹理
  └─ svg2pdf → 单页 PDF
```

### 15.2 目标链路

```text
DWG
  ↓ LibreDWG
DocumentIndex
  ├─ Model Space
  ├─ Layout A
  │    ├─ Paper Space entities
  │    └─ Viewport 1..N
  ├─ Layout B
  └─ Shared blocks / layers / styles / fonts

用户选择 Layout
  ↓
LayoutSceneBuilder
  ├─ Paper Space scene
  ├─ Viewport model scene
  ├─ clipping
  ├─ layer overrides
  └─ text/font resolution

LayoutScene
  ├─ 屏幕渲染
  ├─ 单页 PDF
  └─ 多页 PDF 聚合
```

### 15.3 建议领域模型

```rust
struct CadDocument {
    source: PathBuf,
    layers: LayerTable,
    blocks: BlockTable,
    model_space: SpaceRef,
    layouts: Vec<CadLayout>,
}

struct CadLayout {
    id: LayoutId,
    name: String,
    order: i32,
    paper_size: Size,
    orientation: Orientation,
    paper_space: SpaceRef,
    viewports: Vec<CadViewport>,
}

struct CadViewport {
    id: ViewportId,
    paper_rect: Rect,
    model_center: Point,
    model_height: f64,
    twist_angle: f64,
    clip: ViewportClip,
    frozen_layers: HashSet<LayerId>,
}

struct LayoutScene {
    layout_id: LayoutId,
    bounds: Rect,
    primitives: Vec<Primitive>,
    warnings: Vec<RenderWarning>,
}
```

### 15.4 关键架构调整

1. 将“解析文件”与“构建某个 Layout 场景”分离。
2. 不再将标题启发式范围作为全局唯一场景。
3. Paper Space 和 Model Space 使用独立坐标系统。
4. Viewport 使用变换矩阵和裁剪路径组合。
5. Block 定义与 Block 实例尽量共享，减少重复展开。
6. PDF 生成从“单 SVG”升级为“LayoutScene 列表”。

---

## 16. 版本迭代计划

### v0.1.x：当前基线与限制透明化

目标：

- 保持目标二维图正确打开。
- 保持中文可读。
- 明确告知当前为单场景自动裁剪。

范围：

- 更新 PRD 和 README。
- 状态栏显示单场景限制。
- 检测多个 Layout 并提示。
- 导出前提示仅导出当前场景。
- 保留当前 PDF 转换能力。

不包含：

- Layout 切换。
- 多页 PDF。

发布门槛：

- 当前回归 DWG 显示正确。
- 中文无乱码。
- 不再让用户误认为已经支持全部 Layout。

### v0.2.0：多 Layout 查看 MVP

目标：

- 用户能看到并切换所有 Layout。

范围：

- 解析 Layout 列表、名称和顺序。
- 增加“模型 + Layout”标签栏。
- 支持 Paper Space 基础实体。
- 支持矩形 Viewport。
- 支持 Layout 按需加载。
- 保存每个 Layout 的缩放与中心位置。
- 导出当前 Layout 单页 PDF。
- 命令行增加 `--layout`。

暂不包含：

- 多页 PDF。
- 非矩形 Viewport。
- Viewport 独立冻结图层。
- 完整打印样式。

验收门槛：

- 多 Layout 测试文件的 Layout 数量、名称、顺序正确。
- 任意 Layout 可切换。
- 切换后页面内容不会串页。
- 当前 Layout PDF 与屏幕内容一致。

### v0.3.0：高保真 Viewport 与批量导出

目标：

- 多 Layout 文件可完整查看并一键转为多页 PDF。

范围：

- 多 Viewport。
- Viewport 旋转。
- Viewport 图层冻结覆盖。
- 非矩形 Viewport 裁剪。
- `--all-layouts`。
- 多页 PDF。
- 分 Layout PDF。
- Layout 导出进度与错误汇总。
- 白底/深色背景选项。

验收门槛：

- 全部 Layout 页数正确。
- 每页纸张尺寸与方向正确。
- Viewport 外内容不泄漏。
- 部分 Layout 失败时有明确报告。

### v0.4.0：实体与字体保真增强

目标：

- 提升复杂工程图兼容性。

范围：

- HATCH。
- 更精确的 SPLINE/NURBS。
- 更完整的 DIMENSION。
- SHX 字体映射。
- 缺失字体管理。
- 图层面板。
- XREF 本地文件解析。
- 线宽与基础打印样式。

验收门槛：

- 中文、SHX 替代和标注在回归图中稳定。
- HATCH 不明显拖慢首屏。
- 缺失资源均有可理解警告。

### v0.5.0：性能与批处理

目标：

- 大文件和批量转换可用于生产环境。

范围：

- Block/Symbol 复用，减少图元展开。
- Layout 场景 LRU 缓存。
- 解析任务取消。
- 预取相邻 Layout。
- 转换结果缓存。
- JSON 进度输出。
- 批量目录转换。
- 崩溃日志与性能统计（默认本地，不上传）。

### v1.0.0：稳定版

目标：

- 完成稳定、多 Layout、可批量转换的便携式 DWG 查看器。

发布要求：

- 核心回归集全部通过。
- 无已知数据丢失型导出问题。
- 多 Layout 与多页 PDF 达到稳定标准。
- 中文、字体替代和错误提示完整。
- Windows 10/11 和高 DPI 验证完成。
- 许可证、源码归档和第三方声明完整。

---

## 17. 优先级

### P0：必须完成

- Layout 发现与列表。
- Paper Space 基础渲染。
- 矩形 Viewport。
- Layout 切换。
- 当前 Layout 导出。
- 中文 Layout 名称。
- 明确移除“单一标题区域等同 Layout”的逻辑。

### P1：高优先级

- 多页 PDF。
- 多 Viewport。
- Viewport 图层冻结。
- Layout 懒加载和缓存。
- SHX 字体替代。
- HATCH。

### P2：中优先级

- 非矩形 Viewport。
- 图层面板。
- XREF。
- 线宽和打印样式。
- 批量目录转换。

### P3：低优先级

- 测量工具。
- 缩略图导航。
- 最近文件。
- 文件关联。
- 自动更新。
- 批注功能。

---

## 18. 验收测试计划

### 18.1 回归文件类型

测试集必须至少包含：

1. 单模型空间、无 Layout 的简单 DWG。
2. 模型空间存在多张并排图纸的 DWG。
3. 单 Layout、单 Viewport。
4. 多 Layout、单 Viewport。
5. 多 Layout、多 Viewport。
6. Viewport 旋转。
7. Viewport 冻结图层。
8. 中文 Layout 名称。
9. 中文 TEXT/MTEXT。
10. 深层嵌套块。
11. 大量 INSERT/MINSERT。
12. 缺失 SHX。
13. HATCH 密集图。
14. 损坏或截断 DWG。
15. 中文路径和长路径文件。

### 18.2 当前多 Layout 工业图回归要求

当前用于修复的多图纸工业 DWG 应作为固定回归样本，其验收点包括：

- 五层车间暖通图可正确显示。
- 新增速冻库、备注、标题和图例中文可读。
- 风机和设备块位置正确。
- 图层颜色正确。
- 其他 Layout 可在 v0.2 中被发现和切换。
- v0.3 全部 Layout 导出页数正确。

### 18.3 自动测试

- DXF pair 解析测试。
- Layout 对象解析测试。
- Viewport 变换矩阵测试。
- Viewport 裁剪测试。
- 图层覆盖测试。
- TEXT/MTEXT 格式测试。
- 中文编码测试。
- Block 递归与循环引用测试。
- PDF 页数和页面尺寸测试。
- CLI 参数与退出码测试。

### 18.4 视觉测试

- 对基准 Layout 生成固定尺寸 PNG。
- 与经过人工确认的基准图进行视觉差异比较。
- 对文字、表格、轴网、设备块和页面边界设置重点区域。
- 差异超过阈值时必须人工确认。

### 18.5 性能测试

记录：

- LibreDWG 转换时间。
- DXF 解析时间。
- Layout 索引时间。
- 首个 Layout 场景构建时间。
- Layout 切换时间。
- PDF 导出时间。
- 峰值内存。
- 场景图元数。

---

## 19. 产品指标

### 19.1 核心指标

- DWG 成功打开率。
- Layout 识别正确率。
- 中文文字可读率。
- 当前 Layout PDF 导出成功率。
- 全部 Layout PDF 页数正确率。
- 首屏可交互时间。
- Layout 切换时间。
- 崩溃率。

### 19.2 建议目标

- 常规二维 DWG 成功打开率 ≥ 95%。
- 已支持实体范围内中文可读率 ≥ 99%。
- Layout 名称和数量正确率 ≥ 98%。
- 当前 Layout 导出成功率 ≥ 98%。
- 回归集崩溃率为 0。

---

## 20. 风险与应对

### 风险 R1：LibreDWG 的 Layout/DXF 信息不完整

应对：

- 使用多个 DWG 版本验证。
- 必要时使用 LibreDWG JSON/minJSON 补充对象关系。
- 将 Layout 索引解析与实体渲染解耦。
- 对缺失属性提供降级策略和警告。

### 风险 R2：块展开造成内存和节点爆炸

应对：

- 保留块定义，使用实例化渲染。
- 场景按 Layout 和 Viewport 构建。
- 使用空间索引裁剪。
- 限制递归深度和数组数量。

### 风险 R3：标题启发式选错图

应对：

- v0.2 后标题启发式只用于模型空间书签。
- Layout 选择必须基于真实 LAYOUT 对象。
- UI 明确展示当前空间和 Layout 名称。

### 风险 R4：中文字体与 SHX 不一致

应对：

- 建立字体映射层。
- 显示替代字体警告。
- 提供项目级字体目录。
- 对 PDF 评估嵌入字体或转路径。

### 风险 R5：屏幕与 PDF 不一致

应对：

- 屏幕与 PDF 共用 `LayoutScene`。
- 不为 PDF 单独重新解释实体。
- 视觉回归同时验证屏幕 PNG 与 PDF PNG。

### 风险 R6：GPL 合规

应对：

- 保留 GPL-3.0-or-later 许可证。
- 便携包附带 LibreDWG 对应源码归档。
- 保留第三方声明。
- 新增依赖前检查许可证兼容性。

---

## 21. 待决策事项

1. 默认打开 DWG 保存时的活动 Layout，还是第一个 Paper Space Layout？
2. Model Space 是否默认显示保存视图，还是自动识别最大图纸区域？
3. “导出全部 Layout”是否默认包含 Model Space？
4. 多页 PDF 是否保留每个 Layout 的原始纸张尺寸？
5. PDF 默认使用白底打印色还是保持屏幕颜色？
6. 缺失 SHX 时，是自动替换还是要求用户确认？
7. 是否允许用户配置外部字体目录和 XREF 搜索目录？
8. 缓存放在内存、临时目录，还是与程序同目录？
9. 是否需要支持 Windows 文件关联？
10. v1.0 前是否加入测量距离功能？

建议默认决策：

- 默认打开 DWG 保存时的活动 Layout；不可用时打开第一个可见 Layout。
- Model Space 默认使用保存视图，并提供“显示全部”。
- 全部 Layout 导出默认不包含 Model Space。
- 多页 PDF 保留每个 Layout 的原始纸张尺寸。
- PDF 默认白底打印模式。
- 缺失字体自动替换并显示警告。

---

## 22. Definition of Done

一个多 Layout 版本只有同时满足以下条件才能视为完成：

- 能列出模型空间和全部 Layout。
- Layout 名称、数量、顺序正确。
- 用户可切换任意 Layout。
- Layout 中 Paper Space 和 Viewport 内容同时正确显示。
- 切换 Layout 不阻塞 UI。
- 当前 Layout 导出与屏幕一致。
- 全部 Layout 可输出多页 PDF。
- 中文文字和 Layout 名称无乱码。
- 缺失实体或字体有明确警告。
- 自动测试、视觉测试、性能测试通过。
- README、PRD、许可证和便携包同步更新。

---

## 23. 版本记录

| 文档版本 | 日期 | 说明 |
|---|---|---|
| 1.0 | 2026-07-29 | 建立 PRD 基线；记录 v0.1.x 单视口限制；定义多 Layout、Viewport 和多页 PDF 迭代计划 |

