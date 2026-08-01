Windows 便携版，解压即可运行，无需安装。

- `Cadviewer.exe` — 图形界面查看器与导出器
- `Cadconvert.exe` — 命令行转换器：`Cadconvert.exe input.dwg output.pdf [--mono] [--sheet N]`

下载 `Cadviewer-portable-win64.zip`，解压后直接运行。`Cadviewer-portable-win64.zip.sha256`
可用于校验下载完整性。首次运行时 Windows SmartScreen 可能提示未知发布者——本程序未做
代码签名。

**不附带任何字库。** SHX 属 Autodesk 及第三方授权资产，TrueType 属微软，程序一律在运行时
查找机器上已安装的字库；缺字库时会在警告区逐条写明缺哪个、用了哪个、影响多少实体。

本程序按 GPL-3.0-or-later 发布，内含 GNU LibreDWG 0.14（同为 GPL-3.0-or-later）。
压缩包内 `source\` 目录附带 LibreDWG 对应源码与本程序源码，以履行 GPL 的源码提供义务。
