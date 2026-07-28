# Cadviewer

Cadviewer 是一个面向 Windows 的极简、便携式 DWG 查看器：

- 打开或拖入 `.dwg` / `.dxf`
- 鼠标滚轮缩放、拖动平移、双击适合窗口
- 导出保留矢量几何的 PDF
- 独立轻量命令行转换器：`Cadconvert.exe input.dwg output.pdf`
- GUI 也支持命令行转换：`Cadviewer.exe --convert input.dwg output.pdf`
- 无安装程序、无注册表写入，解压即可运行

## 技术路线

Cadviewer 使用 [GNU LibreDWG](https://github.com/LibreDWG/libredwg) 的
`dwg2dxf` 完成 DWG 解码，再在进程内将常见二维 CAD 实体转换为规范 SVG。
查看器使用 `resvg` 按当前视口栅格化，PDF 则由 `svg2pdf` 直接生成矢量页面。

当前支持的主要实体包括 LINE、CIRCLE、ARC、ELLIPSE、LWPOLYLINE（含 bulge）、
POLYLINE、SPLINE、TEXT、MTEXT、POINT、SOLID、TRACE、3DFACE、LEADER、MLINE、
INSERT/MINSERT、块和常见 DIMENSION 块。三维实体、HATCH、外部参照及部分高级
R2010+ 对象可能被忽略。

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

## 许可证

Cadviewer 按 GPL-3.0-or-later 发布。LibreDWG 0.14 同样使用
GPL-3.0-or-later；发行包附带相应许可证、第三方声明和 LibreDWG 对应源码归档。
