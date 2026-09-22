# MeNote 使用入口（soft 目录）

这是 MeNote 的**正式使用入口**。日常使用只需双击 `MeNote.exe`（或 `启动 MeNote.bat`）。

## 目录内容

| 文件 | 说明 |
|---|---|
| `MeNote.exe` | 发行版单文件程序（约 98 MB，内置 Node 运行时与全部业务代码） |
| `启动 MeNote.bat` | 便捷启动（等同于双击 exe） |

`MeNote.exe` 与 WebView2 缓存不入库（见 `.gitignore`），可由 `packaging\build-release.ps1` 重新构建。

## 使用方式

双击 `MeNote.exe` → 出现 MeNote 窗口 → 自动完成运行环境准备与服务启动 → 进入界面。

- 首次启动或版本更新后：需释放运行环境，约 6~10 秒
- 之后启动：秒开
- 关闭窗口即退出（后台服务随之停止）

## 数据位置（与程序完全分离）

| 位置 | 内容 |
|---|---|
| `%APPDATA%\MeNote\data\` | **用户数据**：笔记数据库、P2P 节点密钥、上传附件 |
| `%LOCALAPPDATA%\MeNote\runtime\` | 程序运行文件（版本更新时自动重建） |
| `%LOCALAPPDATA%\MeNote\server.log` | 服务日志（排查问题用） |
| `%LOCALAPPDATA%\MeNote\launcher.log` | 启动器日志 |

**数据不会因程序更新而丢失**：新版 `MeNote.exe` 覆盖旧文件后重启，
启动器检测到版本变化会自动重建运行环境，用户数据原样加载。

## 版本更新

1. 退出正在运行的 MeNote
2. 用新版 `MeNote.exe` 覆盖本目录下的同名文件
3. 双击启动 → 自动升级，历史数据保留

## 手动访问

除桌面窗口外，也可用浏览器访问：<http://localhost:3107>

默认管理员账号见安装时设置（开发环境为 `admin123` / `admin123`）。
