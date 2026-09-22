# MeNote 桌面发行版打包

用 **Rust + WebView2** 把 MeNote 打包成单个 `MeNote.exe`，支持「覆盖 exe 即升级」，
用户数据与程序文件彻底分离，不受开发分支影响。

## 产物形态

构建产物会自动同步到 **`soft\MeNote.exe`** —— 那是日常使用的正式入口，
构建 → 发布一条龙，无需手动拷贝。

```
MeNote.exe  (约 98 MB，单文件)
├── Rust 启动器（tao + wry / WebView2）     ← 窗口壳 + 运行时托管
└── payload.zip（附加在文件尾部）
    ├── node.exe                             ← 自带运行时，用户无需装 Node
    ├── server.js / app / config / lib / public / node_modules …
    └── （不含 data/ 与 config/lock.js）
```

## 运行布局（与开发目录完全解耦）

| 位置 | 内容 | 更新时 |
|---|---|---|
| `%LOCALAPPDATA%\MeNote\runtime\` | 程序文件（从 payload 释放） | **随版本重建** |
| `%APPDATA%\MeNote\data\` | 用户数据（DB / P2P 密钥 / 上传附件） | **永不覆盖** |
| `%LOCALAPPDATA%\MeNote\server.log` | node 输出 | 每次启动重写 |
| `%LOCALAPPDATA%\MeNote\launcher.log` | 启动器日志 | 追加 |

### 数据分离的实现：目录联接（junction）

```
runtime\data            --JUNCTION-->  %APPDATA%\MeNote\data
runtime\public\upload   --JUNCTION-->  %APPDATA%\MeNote\data\upload
```

用 junction 而非改代码的好处：`config/db.js`、`lib/p2p.js`、上传控制器写的仍是
`<应用根>/data`、`<应用根>/public/upload`，但被系统透明重定向到数据目录——
**零源码侵入**，开发分支无需为打包做任何妥协。junction 不需要管理员权限。

## 版本更新机制

1. 新版 `MeNote.exe` 覆盖旧文件
2. 启动时读自身尾部内嵌的版本号，与 `runtime\.version` 比对
3. 不一致 → 先摘除两个 junction（防穿透删数据）→ 删除旧 runtime → 重新释放
4. 版本号一致 → 跳过释放，秒起

`config/lock.js`（"已安装"标记）位于程序目录，重建后会丢失。
启动器会检查：**数据目录已有数据库但锁文件缺失时自动补写**，避免已安装用户被要求重新安装。

## 构建

前置：Rust 工具链、Node.js（用于取 node.exe 与读取版本号）。

```powershell
cd packaging
.\build-release.ps1                    # 完整构建
.\build-release.ps1 -Version 1.1.0     # 指定版本号
.\build-release.ps1 -SkipBuild         # 复用已编译的启动器，仅重打包 payload
```

产物：`packaging\build\MeNote.exe`

构建脚本会做以下校验（防呆）：
- `config/lock.js` 不得进包（否则新用户跳过安装向导）
- `data\` 不得进包（用户数据）
- 关键文件齐全（`node.exe` / `server.js` / `config\db.js` / 管理后台模板）

## 尾部格式（脚本与启动器必须一致）

```
[ 裸启动器 exe ][ payload.zip ][ version ][ ver_len:u32 ][ zip_len:u64 ][ magic:16 ]
```

`magic = "MENOTE_PKG_v1\0\0\0"`。启动器从文件末尾倒着读 28 字节 footer 定位 payload。
改格式需同时改 `launcher/src/main.rs` 常量与 `build-release.ps1` 的 `$MAGIC`。

### 更新用户端：覆盖 exe 的正确姿势

1. 退出正在运行的 MeNote（关窗口即可；否则 exe 被锁无法覆盖）
2. 用新版 MeNote.exe 覆盖旧文件
3. 双击启动 → 自动检测版本差异 → 重建 runtime → 历史数据原样加载

构建脚本自身的限制：build-release.ps1 输出到 packaging\build\MeNote.exe。
若该文件正被运行中的实例占用，脚本会明确报错提示先退出，而不是抛出 IOException。

### 数据保留的边界

保留（位于 %APPDATA%\MeNote\data）：
- 笔记数据库 menote.db
- P2P 节点密钥 p2p-key.bin（节点 ID 跨版本不变）
- 上传的附件 upload\

不保留（随版本重建）：程序文件、config/lock.js（由启动器按数据目录状态自动补写）

## 启动器行为

- **单实例复用**：端口已有服务时直接连接，不重复启动
- **进程守护**：node 子进程加入 Job Object（`KILL_ON_JOB_CLOSE`），
  启动器正常退出或被强杀，node 都会被系统一并终止，不留孤儿进程
- **加载页**：释放/启动期间窗口显示进度（每 10% 更新），失败显示错误页而非白屏
- **探活**：轮询 HTTP 直到就绪（最长 180 秒）；node 提前退出则立即报错并指向日志

## 源码结构

```
packaging/
├── build-release.ps1        # 构建脚本
├── launcher/
│   ├── Cargo.toml           # tao 0.24 + wry 0.39 + zip + windows-sys 0.52
│   ├── build.rs             # 从 package.json 生成 APP_VERSION
│   └── src/main.rs          # 启动器（payload 解析 / runtime 释放 / junction / Job Object）
└── build/                   # 产物（已 gitignore）
```

## 依赖版本说明

`tao 0.24` + `wry 0.39` 是 Tauri 1.6 同期的配对组合。选它而非最新版的原因：
更新的 tao 0.30 已跟进 winit 0.30 的新 API（`EventLoopBuilder`、`Size` 枚举），
而中间版本存在 `gdk-sys` / `javascriptcore-rs` 的上游 links 冲突，无法解析依赖。

## 已知限制

- **仅 Windows**：依赖 WebView2。`main.rs` 里 `Job Object`、`GetLocalTime`、`mklink` 均为 Win32 API
- **WebView2 Runtime 依赖**：Win10 1803+ / Win11 通常已内置；缺失时启动器弹窗给出下载链接
- **首次启动较慢**：需释放约 317 MB（压缩后 97 MB），实测约 6~10 秒
