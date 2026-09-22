// MeNote 桌面发行版启动器
// ----------------------------------------------------------------------------
// 单文件发行模式：MeNote.exe = Rust 启动器 + 附加在尾部的 payload.zip
// （payload 含 node.exe 与项目全量文件，由 packaging/build-release.ps1 拼接）。
//
// 运行布局（与开发目录完全解耦）：
//   %LOCALAPPDATA%\MeNote\runtime\     程序文件（随版本重建，不含用户数据）
//   %APPDATA%\MeNote\data\             用户数据（DB / P2P 密钥 / 上传附件，永不覆盖）
//   %LOCALAPPDATA%\MeNote\server.log   node 子进程输出
//   %LOCALAPPDATA%\MeNote\launcher.log 启动器日志
//
// 数据分离通过 Windows junction 实现（零源码侵入）：
//   runtime\data            -> %APPDATA%\MeNote\data
//   runtime\public\upload   -> %APPDATA%\MeNote\data\upload
//   config/db.js、lib/p2p.js、上传控制器写这些路径时被透明重定向到数据目录。
//
// 版本更新：新版 MeNote.exe 覆盖旧 exe → 内嵌版本与 runtime\.version 不一致
// → 启动器删除旧 runtime 并重新释放（先摘 junction 防穿透），用户数据不动。
// ----------------------------------------------------------------------------

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    window::Window,
};

include!(concat!(env!("OUT_DIR"), "/version.rs"));
// 窗口图标像素（build.rs 从 assets/icon-256.png 解码生成，缺失时为 None）
include!(concat!(env!("OUT_DIR"), "/window_icon.rs"));
const PORT: u16 = 3107;
const MAGIC: &[u8; 16] = b"MENOTE_PKG_v1\0\0\0";
/// 尾部固定 footer：ver_len(u32) + zip_len(u64) + magic(16) = 28 字节
const FOOTER_LEN: u64 = 28;

/// 探活窗口：首次释放 runtime + 首次建库可能较慢
const PROBE_TIMEOUT: Duration = Duration::from_secs(180);
/// 单次 HTTP 探活的读超时
const PROBE_ONCE: Duration = Duration::from_millis(800);
/// 探活轮询间隔
const PROBE_INTERVAL: Duration = Duration::from_millis(350);

/// 后台线程 → UI 线程的消息
enum Msg {
    /// 更新 loading 页状态文字
    Status(String),
    /// 服务就绪，导航到该 URL
    Ready(String),
    /// 不可恢复错误：窗口内显示错误页
    Fatal(String),
}

struct Dirs {
    local: PathBuf,   // %LOCALAPPDATA%\MeNote
    runtime: PathBuf, // %LOCALAPPDATA%\MeNote\runtime
    data: PathBuf,    // %APPDATA%\MeNote\data
    server_log: PathBuf,
    launcher_log: PathBuf,
}

fn main() {
    let dirs = match resolve_dirs() {
        Ok(d) => d,
        Err(e) => {
            fatal_msgbox(&format!("初始化目录失败：{e}"));
            std::process::exit(1);
        }
    };
    log(&dirs, &format!("== MeNote 启动器 v{APP_VERSION} =="));

    // 读取附加在自身尾部的 payload（zip 范围 + 打包版本）
    let pkg = match read_pkg_footer() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!(
                "读取发行包失败：{e}\n\n请确认 MeNote.exe 来自完整发行包，不要单独截取。"
            );
            log(&dirs, &msg);
            fatal_msgbox(&msg);
            std::process::exit(1);
        }
    };
    log(
        &dirs,
        &format!("payload: v{}，zip {} MB", pkg.version, pkg.zip_len / 1048576),
    );

    // 版本一致 → 数据库存在 → 直接进后台；其余情况窗口先亮 loading
    let already_installed = dirs.data.join("menote.db").exists();

    // ---- UI：窗口 + loading 页 ----
    let event_loop: EventLoop<Msg> = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = match Window::new(&event_loop) {
        Ok(w) => w,
        Err(e) => {
            let msg = format!("创建窗口失败：{e}\n\n若提示 WebView2 相关错误，请安装 WebView2 Runtime：\nhttps://developer.microsoft.com/microsoft-edge/webview2/");
            log(&dirs, &msg);
            fatal_msgbox(&msg);
            std::process::exit(1);
        }
    };
    window.set_title("MeNote");
    // 窗口图标（标题栏左上角 + 任务栏）。exe 资源节的图标只影响文件显示，
    // 运行中的窗口需显式设置，否则任务栏用的是默认图标。
    apply_window_icon(&window);
    use tao::dpi::{LogicalSize, Size};
    window.set_inner_size(Size::Logical(LogicalSize::new(1280f64, 840f64)));
    window.set_min_inner_size(Some(Size::Logical(LogicalSize::new(960f64, 600f64))));
    // WebView2 内存优化：wry 0.39 未暴露传 Chromium 命令行参数的 API，
    // 改用 WebView2 官方支持的环境变量（必须在创建 WebView 之前设置，否则不生效）。
    // 默认配置会常驻多进程 + 后台定时器 + 各类预加载，实测占 500MB+；
    // 以下参数在保持功能完整的前提下削减常驻内存。
    apply_webview2_memory_tuning();

    let webview = match wry::WebViewBuilder::new(&window)
        .with_html(loading_page("正在启动 MeNote…"))
        .build()
    {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("初始化 WebView2 失败：{e}\n\n请安装 WebView2 Runtime 后重试：\nhttps://developer.microsoft.com/microsoft-edge/webview2/");
            log(&dirs, &msg);
            fatal_msgbox(&msg);
            std::process::exit(1);
        }
    };

    // node 子进程句柄：主线程持有，退出前确保清理
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let exited = Arc::new(AtomicBool::new(false));

    // ---- 后台线程：runtime 保障 → 启 node → 探活 ----
    {
        let proxy = proxy.clone();
        let dirs = clone_dirs(&dirs);
        let pkg_start = pkg.zip_start;
        let pkg_len = pkg.zip_len;
        let pkg_version = pkg.version.clone();
        let child_slot = child_slot.clone();
        thread::spawn(move || {
            if let Err(e) = worker(
                &dirs,
                pkg_start,
                pkg_len,
                &pkg_version,
                &proxy,
                &child_slot,
                already_installed,
            ) {
                let _ = proxy.send_event(Msg::Fatal(format!(
                    "{}\n\n详细信息见：\n{}\n{}",
                    e,
                    dirs.launcher_log.display(),
                    dirs.server_log.display()
                )));
            }
        });
    }

    // 事件循环必须独占 webview（wry 非 Send），用 main 线程闭包持有
    let webview = Some(webview);
    let child_slot = child_slot;
    let exited = exited;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(msg) => match msg {
                Msg::Status(text) => {
                    if let Some(wv) = webview.as_ref() {
                        let js = format!(
                            "var el=document.getElementById('status');if(el)el.textContent={};",
                            js_str(&text)
                        );
                        let _ = wv.evaluate_script(&js);
                    }
                }
                Msg::Ready(url) => {
                    if let Some(wv) = webview.as_ref() {
                        let js = format!("window.location.replace({})", js_str(&url));
                        let _ = wv.evaluate_script(&js);
                    }
                }
                Msg::Fatal(text) => {
                    if let Some(wv) = webview.as_ref() {
                        let _ = wv.evaluate_script(&format!(
                            "document.documentElement.innerHTML={}",
                            js_str(&error_page(&text))
                        ));
                    }
                }
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // 关窗 = 退出：先杀 node（Job Object 兜底崩溃场景）
                exited.store(true, Ordering::SeqCst);
                if let Ok(mut slot) = child_slot.lock() {
                    if let Some(child) = slot.as_mut() {
                        let _ = child.kill();
                    }
                    *slot = None;
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::LoopDestroyed => {
                // 兜底：事件循环销毁时再次确保 node 已终止
                if !exited.load(Ordering::SeqCst) {
                    if let Ok(mut slot) = child_slot.lock() {
                        if let Some(child) = slot.as_mut() {
                            let _ = child.kill();
                        }
                        *slot = None;
                    }
                }
            }
            _ => {}
        }
    });
}

// ----------------------------------------------------------------------------
// 后台工作流
// ----------------------------------------------------------------------------

/// 后台线程主体：确保 runtime 就绪 → 启动/复用服务 → 探活 → Ready
fn worker(
    dirs: &Dirs,
    pkg_start: u64,
    pkg_len: u64,
    pkg_version: &str,
    proxy: &tao::event_loop::EventLoopProxy<Msg>,
    child_slot: &Arc<Mutex<Option<Child>>>,
    already_installed: bool,
) -> Result<(), String> {
    let url = |installed: bool| {
        if installed {
            format!("http://localhost:{PORT}/admin")
        } else {
            format!("http://localhost:{PORT}/install")
        }
    };

    // 1) 端口已有服务（二次启动/残留实例）→ 直接复用
    if probe_once() {
        let _ = proxy.send_event(Msg::Status(
            "检测到 MeNote 服务已在运行，正在连接…".to_string(),
        ));
        thread::sleep(Duration::from_millis(400));
        let _ = proxy.send_event(Msg::Ready(url(already_installed)));
        return Ok(());
    }

    // 2) 保障 runtime（版本不一致则重建）
    ensure_runtime(dirs, pkg_start, pkg_len, pkg_version, proxy)?;

    // 3) 数据 junction + 安装锁兜底
    ensure_junctions(dirs)?;
    let installed_now = dirs.data.join("menote.db").exists();
    ensure_lock_js(dirs, installed_now);

    // 4) 启动 node
    let _ = proxy.send_event(Msg::Status("正在启动服务…".to_string()));
    let child = spawn_node(dirs)?;
    install_job_object(&child)?;
    child_slot
        .lock()
        .map_err(|_| "内部状态锁损坏".to_string())?
        .replace(child);

    // 5) 探活
    let started = Instant::now();
    while started.elapsed() < PROBE_TIMEOUT {
        if probe_once() {
            let _ = proxy.send_event(Msg::Ready(url(installed_now)));
            return Ok(());
        }
        // node 崩溃则提前失败，别让用户干等
        if let Ok(mut slot) = child_slot.lock() {
            if let Some(c) = slot.as_mut() {
                if let Ok(status) = c.try_wait() {
                    if status.is_some() {
                        return Err(format!(
                            "服务进程启动即退出（详见 server.log 尾部），退出码：{:?}",
                            status.map(|s| s.code().unwrap_or(-1))
                        ));
                    }
                }
            }
        }
        thread::sleep(PROBE_INTERVAL);
    }
    Err("服务未能在限定时间内就绪（可能端口被占用或运行环境异常）".to_string())
}

// ----------------------------------------------------------------------------
// 自身 payload 读取
// ----------------------------------------------------------------------------

struct Pkg {
    zip_start: u64,
    zip_len: u64,
    version: String,
}

/// 读取 exe 尾部 footer，定位内嵌 payload.zip 与打包版本。
/// 尾部布局（文件顺序）：zip | version | ver_len(u32) | zip_len(u64) | magic(16)
fn read_pkg_footer() -> Result<Pkg, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut f = fs::File::open(&exe).map_err(|e| format!("打开自身失败：{e}"))?;
    let total = f.metadata().map_err(|e| e.to_string())?.len();
    if total < FOOTER_LEN {
        return Err("文件过小，缺少发行数据".to_string());
    }

    // 读尾部 28 字节：[ver_len(4) | zip_len(8) | magic(16)]
    f.seek(SeekFrom::End(-(FOOTER_LEN as i64)))
        .map_err(|e| e.to_string())?;
    let mut footer = vec![0u8; FOOTER_LEN as usize];
    f.read_exact(&mut footer).map_err(|e| e.to_string())?;

    let ver_len = u32::from_le_bytes(footer[0..4].try_into().unwrap()) as u64;
    let zip_len = u64::from_le_bytes(footer[4..12].try_into().unwrap());
    if &footer[12..28] != MAGIC {
        return Err("发行包格式校验失败（magic 不匹配）".to_string());
    }
    if zip_len == 0 || total < FOOTER_LEN + ver_len + zip_len {
        return Err("发行包数据不完整".to_string());
    }
    // 读完 footer 后文件位置在末尾（total）；version 位于 footer 之前，
    // 故需回退「footer 长度 + version 长度」才能对齐到 version 起点
    let zip_start = total - FOOTER_LEN - ver_len - zip_len;
    f.seek(SeekFrom::Current(-(FOOTER_LEN as i64 + ver_len as i64)))
        .map_err(|e| e.to_string())?;
    let mut ver = vec![0u8; ver_len as usize];
    f.read_exact(&mut ver).map_err(|e| e.to_string())?;
    let version = String::from_utf8(ver).map_err(|_| "版本号编码损坏".to_string())?;

    Ok(Pkg {
        zip_start,
        zip_len,
        version,
    })
}

// ----------------------------------------------------------------------------
// runtime 保障（版本检查 + 释放）
// ----------------------------------------------------------------------------

fn runtime_version_marker(dirs: &Dirs) -> PathBuf {
    dirs.runtime.join(".version")
}

/// runtime\.version 与 payload 版本一致且结构完整 → 跳过释放；
/// 否则删除重建（先摘 junction，避免任何穿透删除数据的风险）。
fn ensure_runtime(
    dirs: &Dirs,
    pkg_start: u64,
    pkg_len: u64,
    pkg_version: &str,
    proxy: &tao::event_loop::EventLoopProxy<Msg>,
) -> Result<(), String> {
    fs::create_dir_all(&dirs.local).map_err(|e| e.to_string())?;
    fs::create_dir_all(&dirs.data).map_err(|e| e.to_string())?;

    // 版本号由调用方从 payload footer 解析后传入（单一数据源），
    // 此处不再自行解析尾部——重复实现偏移量极易出错
    let pkg = pkg_version.to_string();
    let marker = runtime_version_marker(dirs);
    let marker_ok = marker.exists()
        && dirs.runtime.join("server.js").exists()
        && match fs::read_to_string(&marker) {
            Ok(s) => s.trim() == pkg,
            Err(_) => false,
        };
    if marker_ok {
        return Ok(());
    }

    let _ = proxy.send_event(Msg::Status(format!(
        "正在准备运行环境（版本 {pkg}）…首次或更新后约需 10~60 秒"
    )));

    // 旧 runtime 清理：先摘两个 junction，再整目录删除
    if dirs.runtime.exists() {
        detach_junction(&dirs.runtime.join("data"));
        detach_junction(&dirs.runtime.join("public").join("upload"));
        let backup = dirs.local.join("runtime_old");
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        fs::rename(&dirs.runtime, &backup)
            .or_else(|_| fs::remove_dir_all(&dirs.runtime).map(|_| ()))
            .map_err(|e| format!("清理旧运行环境失败：{e}"))?;
        let _ = fs::remove_dir_all(&backup);
    }
    fs::create_dir_all(&dirs.runtime).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // 释放 payload.zip
    extract_payload(&exe, pkg_start, pkg_len, &dirs.runtime, proxy)?;

    // 写版本标记（最后写：中途断电/崩溃 → 下次启动会重来，保持原子语义）
    fs::write(&marker, &pkg).map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_payload(
    exe: &Path,
    zip_start: u64,
    zip_len: u64,
    dest: &Path,
    proxy: &tao::event_loop::EventLoopProxy<Msg>,
) -> Result<(), String> {
    let mut f = File::open(exe).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(zip_start)).map_err(|e| e.to_string())?;
    let stream = f.take(zip_len);
    let mut archive =
        zip::ZipArchive::new(stream).map_err(|e| format!("读取 payload.zip 失败：{e}"))?;

    let total = archive.len();
    let mut last_reported = 0u64;
    for i in 0..total {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩条目失败：{e}"))?;

        // 防 path traversal：enclosed_name 已做规范化与根逃逸检查
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let out_path = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = File::create(&out_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;

        // 每 10% 报一次进度
        let pct = (i as u64 + 1) * 100 / total as u64;
        if pct / 10 > last_reported {
            last_reported = pct / 10;
            let _ = proxy.send_event(Msg::Status(format!(
                "正在释放运行环境… {pct}%（首次或更新后约需 10~60 秒）"
            )));
        }
    }
    Ok(())
}
// ----------------------------------------------------------------------------
// 数据 junction 与安装锁
// ----------------------------------------------------------------------------

/// 建立数据 junction：
///   runtime\data          -> %APPDATA%\MeNote\data
///   runtime\public\upload -> %APPDATA%\MeNote\data\upload
/// junction（目录联接）无需管理员权限；db.js / p2p.js / 上传路径零改动。
fn ensure_junctions(dirs: &Dirs) -> Result<(), String> {
    let upload = dirs.data.join("upload");
    fs::create_dir_all(&upload).map_err(|e| e.to_string())?;

    make_junction(&dirs.runtime.join("data"), &dirs.data)?;
    make_junction(&dirs.runtime.join("public").join("upload"), &upload)?;
    Ok(())
}

fn make_junction(link: &Path, target: &Path) -> Result<(), String> {
    if link.exists() || link.symlink_metadata().is_ok() {
        // 已存在：验证指向正确，不正确则重建
        if is_junction_to(link, target) {
            return Ok(());
        }
        detach_junction(link);
    }
    let out = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags_no_window()
        .output()
        .map_err(|e| format!("调用 mklink 失败：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "建立数据目录联接失败：{}\n目标：{} -> {}",
            String::from_utf8_lossy(&out.stderr),
            link.display(),
            target.display()
        ));
    }
    Ok(())
}

/// 只删 junction 本身，不碰目标内容（remove_dir 对 reparse point 语义安全）
fn detach_junction(path: &Path) {
    if path.symlink_metadata().is_ok() {
        let _ = fs::remove_dir(path);
    }
}

fn is_junction_to(link: &Path, target: &Path) -> bool {
    // Windows：junction 的 symlink_metadata 会命中 reparse point；
    // fs::canonicalize 穿透后与目标 canonicalize 比对
    match (fs::canonicalize(link), fs::canonicalize(target)) {
        (Ok(l), Ok(t)) => l == t,
        _ => false,
    }
}

/// config/lock.js 是程序目录里的安装标记，版本重建后会丢失。
/// 数据目录已有数据库（即已安装过）而锁缺失时，由启动器代写，避免重复走向导。
fn ensure_lock_js(dirs: &Dirs, installed: bool) {
    if !installed {
        return;
    }
    let lock = dirs.runtime.join("config").join("lock.js");
    if lock.exists() {
        return;
    }
    let content = format!(
        "// 本文件标识系统已安装，不可删除。\nmodule.exports = {{\n    install: true,\n    version: '{APP_VERSION}'\n}};\n"
    );
    let _ = fs::write(&lock, content);
}

// ----------------------------------------------------------------------------
// node 子进程
// ----------------------------------------------------------------------------

fn spawn_node(dirs: &Dirs) -> Result<Child, String> {
    let node = dirs.runtime.join("node.exe");
    if !node.exists() {
        return Err("运行环境不完整：缺少 node.exe".to_string());
    }

    // server.log 截断写入，便于排查本次启动问题
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&dirs.server_log)
        .map_err(|e| format!("打开日志失败：{e}"))?;

    Command::new(&node)
        .arg("server.js")
        .current_dir(&dirs.runtime)
        .stdout(Stdio::from(log_file.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log_file))
        .creation_flags_no_window()
        .spawn()
        .map_err(|e| format!("启动 node 失败：{e}"))
}

/// 把子进程纳入 Job Object（KILL_ON_JOB_CLOSE）：
/// 启动器无论正常退出还是崩溃，node 都会随 Job 句柄关闭被系统终止，不留孤儿进程。
fn install_job_object(child: &Child) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job == 0 {
            return Err("CreateJobObject 失败".to_string());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            return Err("SetInformationJobObject 失败".to_string());
        }
        let handle = child.as_raw_handle() as _;
        if AssignProcessToJobObject(job, handle) == 0 {
            return Err("AssignProcessToJobObject 失败".to_string());
        }
        // 故意不 CloseHandle：句柄保持到进程结束，即"启动器活着 node 才活"
    }
    Ok(())
}

// ----------------------------------------------------------------------------
// 探活（零依赖 HTTP/1.0 探测）
// ----------------------------------------------------------------------------

fn probe_once() -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(
        &format!("127.0.0.1:{PORT}").parse().unwrap(),
        PROBE_ONCE,
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(PROBE_ONCE));
    if s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = [0u8; 8];
    // 读到 "HTTP/1.x" 即认为服务在
    matches!(s.read(&mut buf), Ok(n) if n >= 4 && &buf[..4] == b"HTTP")
}

// ----------------------------------------------------------------------------
// 杂项
// ----------------------------------------------------------------------------

/// 设置窗口图标（标题栏左上角 + 任务栏）。
///
/// exe 资源节里的图标只决定文件在资源管理器中的显示；
/// 运行中的窗口若不显式设置，任务栏会退回系统默认图标。
///
/// 像素数据由 build.rs 从 assets/icon-256.png 解码后内嵌（见 window_icon.rs），
/// 运行时无需解码 PNG，也不依赖 image crate。
fn apply_window_icon(window: &Window) {
    if let Some((rgba, w, h)) = WINDOW_ICON_RGBA {
        match tao::window::Icon::from_rgba(rgba.to_vec(), w, h) {
            Ok(icon) => window.set_window_icon(Some(icon)),
            Err(e) => {
                // 图标失败不影响使用，仅记录（无 dirs 上下文，直接 stderr）
                eprintln!("[icon] 窗口图标设置失败: {e:?}");
            }
        }
    }
}

/// 通过 WebView2 环境变量注入 Chromium 命令行参数，降低常驻内存。
/// 为什么用环境变量：wry 0.39 的 Windows 扩展只暴露 `with_browser_accelerator_keys`，
/// 没有传命令行参数的接口（更高版本才有 `with_additional_browser_args`）。
///
/// 必须在创建 WebView **之前** 调用，创建后再设置不会生效。
///
/// 各参数作用：
///   renderer-process-limit=1  限制渲染进程数（默认按站点分进程，内存翻倍）
///   disable-features=...      关闭翻译/媒体路由/模型下载等本应用用不到的后台服务
///   disable-extensions        无扩展，省掉扩展宿主进程
///   disable-component-update  不做组件热更新（本地应用无需）
///   disable-sync / no-first-run / no-default-browser-check  关闭同步与首启检查
///   js-flags=--lite-mode      V8 精简模式，降低 JS 堆与编译缓存占用
fn apply_webview2_memory_tuning() {
    let args = concat!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,",
        "CalculateNativeWinOcclusion,BackForwardCache,Translate,",
        "MediaRouter,OptimizationGuideModelDownloading ",
        "--disable-extensions ",
        "--disable-component-update ",
        "--disable-domain-reliability ",
        "--disable-sync ",
        "--no-first-run ",
        "--no-default-browser-check ",
        "--renderer-process-limit=1 ",
        "--js-flags=--lite-mode"
    );
    // 仅在用户未自行设置时注入，避免覆盖高级用户的自定义配置
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", args);
    }
}

fn resolve_dirs() -> Result<Dirs, String> {
    let local_root = std::env::var("LOCALAPPDATA")
        .map_err(|_| "未设置 LOCALAPPDATA".to_string())?;
    let roaming_root =
        std::env::var("APPDATA").map_err(|_| "未设置 APPDATA".to_string())?;
    let local = Path::new(&local_root).join("MeNote");
    let data = Path::new(&roaming_root).join("MeNote").join("data");
    Ok(Dirs {
        runtime: local.join("runtime"),
        server_log: local.join("server.log"),
        launcher_log: local.join("launcher.log"),
        local,
        data,
    })
}

fn clone_dirs(d: &Dirs) -> Dirs {
    Dirs {
        local: d.local.clone(),
        runtime: d.runtime.clone(),
        data: d.data.clone(),
        server_log: d.server_log.clone(),
        launcher_log: d.launcher_log.clone(),
    }
}

fn log(dirs: &Dirs, msg: &str) {
    use std::fmt::Write as _;
    let mut line = String::new();
    let _ = write!(line, "[{}] {}\n", timestamp(), msg);
    // 父目录可能尚未创建（首次运行），先确保存在，否则日志会静默丢失
    if let Some(parent) = dirs.launcher_log.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&dirs.launcher_log)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 可读时间戳（不引 chrono，用 GetLocalTime 转成 yyyy-MM-dd HH:mm:ss）
fn timestamp() -> String {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

fn fatal_msgbox(msg: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = "MeNote"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
            MessageBoxW(
            0,
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// JS 字符串字面量转义（供 evaluate_script 注入）
fn js_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn loading_page(status: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>MeNote</title><style>
body{{margin:0;height:100vh;display:flex;align-items:center;justify-content:center;
background:#f5f7fa;font-family:'Segoe UI','Microsoft YaHei',sans-serif}}
.card{{text-align:center;padding:40px 56px;background:#fff;border-radius:12px;
box-shadow:0 8px 32px rgba(0,0,0,.08)}}
.logo{{font-size:34px;font-weight:700;color:#409eff;letter-spacing:1px}}
.spin{{margin:18px auto 14px;width:28px;height:28px;border-radius:50%;
border:3px solid #e4e7ed;border-top-color:#409eff;animation:r 0.9s linear infinite}}
@keyframes r{{to{{transform:rotate(360deg)}}}}
#status{{color:#909399;font-size:13px}}
</style></head><body><div class="card"><div class="logo">MeNote</div>
<div class="spin"></div><div id="status">{}</div></div></body></html>"#,
        status
    )
}

fn error_page(text: &str) -> String {
    let esc: String = text
        .chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            c => c.to_string(),
        })
        .collect();
    let page = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>MeNote 启动失败</title><style>
body{{margin:0;height:100vh;display:flex;align-items:center;justify-content:center;
background:#f5f7fa;font-family:'Segoe UI','Microsoft YaHei',sans-serif}}
.card{{max-width:640px;padding:36px 44px;background:#fff;border-radius:12px;
box-shadow:0 8px 32px rgba(0,0,0,.08)}}
h1{{font-size:19px;color:#f56c6c;margin:0 0 14px}}
pre{{white-space:pre-wrap;word-break:break-all;background:#f8f9fb;padding:14px;
border-radius:8px;font-size:13px;color:#606266;border:1px solid #ebeef5}}
</style></head><body><div class="card"><h1>启动失败</h1><pre>{}</pre></div></body></html>"#,
        esc
    );
    // 整个文档作为 innerHTML 注入，需要包一层：直接返回完整 html 时
    // documentElement.innerHTML 会丢 doctype，无碍显示
    page
}

/// CREATE_NO_WINDOW 常量包装，避免在调用点写魔数
trait CreateNoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}
impl CreateNoWindow for Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000)
    }
}
