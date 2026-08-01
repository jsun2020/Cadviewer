# Cadviewer

Cadviewer 是一个面向 Windows 的极简、便携式 DWG 查看器与 PDF 转换器：

- 打开或拖入 `.dwg` / `.dxf`
- 鼠标滚轮缩放、拖动平移、双击适合窗口
- 自动识别模型空间中的图框，一个图框一页，按阅读顺序分页
- 导出保留矢量几何的多页 PDF，线宽、颜色、线型对标 AutoCAD「打印到 PDF」
- 彩色 / 单色（等效 monochrome.ctb）一键切换
- 独立轻量命令行转换器：`Cadconvert.exe input.dwg output.pdf [--mono] [--sheet N]`
- GUI 也支持命令行转换：`Cadviewer.exe --convert input.dwg output.pdf`
- 无安装程序、无注册表写入，解压即可运行

## 技术路线

Cadviewer 使用 [GNU LibreDWG](https://github.com/LibreDWG/libredwg) 的
`dwg2dxf` 完成 DWG 解码（ASCII DXF），随后在进程内走一条以**纸面毫米**为单位的
打印管线：

```
DXF 字节
  -> encoding      按 $DWGCODEPAGE 逐字符串解码（UTF-8 优先，GBK/Big5 兜底）
  -> dxf           lexer + 强类型 LAYER / LTYPE / BLOCK / 实体记录
  -> doc::Document
  -> sheets        图框识别（块名 -> 图层名 -> 几何启发式三级信号）
  -> plot          解析颜色 / 线宽 / 线型，展开块，变换到纸面毫米
  -> PlotScene     所有坐标与宽度均已是毫米
       |
       +-- render::skia  屏幕栅格化（tiny-skia）
       +-- render::pdf   矢量 PDF（pdf-writer，Flate 压缩）
```

关键设计：**所有打印语义决策只发生在 `plot` 层，两个渲染后端只负责画**。
屏幕显示与导出 PDF 由同一份 `PlotScene` 产生，结构上不可能不一致。

线宽以毫米存储，因此 0.35 mm 的线在任何缩放比例下打印出来都是 0.35 mm；
颜色使用精确的 256 项 AutoCAD ACI 查色表（而非近似公式），ACI 7 / 白色在白纸上转为黑色。

当前支持的主要实体包括 LINE、CIRCLE、ARC、ELLIPSE、LWPOLYLINE（含 bulge）、
POLYLINE（含 VERTEX 序列）、SPLINE、POINT、SOLID、TRACE、3DFACE、LEADER、
INSERT / MINSERT、块递归、DIMENSION 几何块，以及文字 TEXT / MTEXT / ATTRIB。

文字直接解析机器上已安装的 SHX 笔画字库（含 CJK 大字体）与 TrueType 字库，
**程序自身不附带任何字库**——SHX 属 Autodesk 及第三方授权资产，一律在运行时
就地查找。找不到原字库时按替代链顶替，并在警告区逐条写明"缺哪个、用了哪个、
影响多少个实体"，绝不静默替换。可用 `--font-dir` 追加搜索目录。

TrueType 绘制的文字在导出的 PDF 里是**真正的文字**：只把用到的字形做成子集内嵌
（`Identity-H` + `ToUnicode`），可选中、可复制、可全文检索。SHX 是笔画字库，没有
可嵌入的字形轮廓字体，因此仍以线条绘制——形状正确，但不可检索。

**尚未实现**（见 `prd.md` 与实现计划中的后续阶段）：

- HATCH 填充、MLINE、外部参照 XREF、三维实体、代理对象
- CTB/STB 打印样式表读取、PDF 图层（OCG）

未绘制的实体类型会在警告区按类型与数量列出，不会被静默丢弃。

## 已知限制

- 大图纸导出较慢：26 页的样例图纸约需 9 分钟。原因是块展开尚未按页做包围盒剔除，
  性能优化属后续阶段。
- 图幅由图框尺寸按 A 系列 fit 推导，可能与 AutoCAD 当时选用的纸张不同（几何等价）。

## 下载

[**下载最新便携版**](https://github.com/jsun2020/Cadviewer/releases/latest)
— 解压 `Cadviewer-portable-win64.zip` 后直接运行 `Cadviewer.exe`，无需安装，
不写注册表。同页的 `.sha256` 可校验下载完整性。

首次运行时 Windows SmartScreen 可能提示未知发布者：本程序未做代码签名。

压缩包内已包含 DWG 解码所需的 LibreDWG 运行时；**不含任何字库**，
文字使用机器上已安装的 SHX / TrueType 字库绘制。

## 开发与构建

需要 Rust stable（MSVC target）：

```powershell
.\scripts\prepare-libredwg.ps1
cargo test
cargo build --release
.\scripts\package.ps1
```

便携包输出到 `dist\Cadviewer-portable-win64`。批处理场景优先使用
`Cadconvert.exe`，它不会初始化 GUI/OpenGL，启动开销更低。

回归测试中的真实图纸与 AutoCAD 参考件属客户资料，**不入版本库**；
相关测试在参考件缺失时会明确打印跳过原因，而不会静默通过。

## 许可证

Cadviewer 按 GPL-3.0-or-later 发布。LibreDWG 0.14 同样使用
GPL-3.0-or-later；发行包附带相应许可证、第三方声明和 LibreDWG 对应源码归档。
