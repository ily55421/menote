// 构建期工作：
//   1. 从 package.json 读取版本号 → 生成 APP_VERSION 常量
//      （发行版版本以此为准：版本变化 → 启动器判定需重新释放 runtime）
//   2. 把 assets/menote.ico 嵌入 exe 资源节 → 资源管理器/任务栏显示应用图标
use std::{env, fs, path::Path};

fn main() {
    // package.json 变化时重新生成
    println!("cargo:rerun-if-changed=../../package.json");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let pkg = Path::new(&manifest_dir).join("../../package.json");
    let raw = fs::read_to_string(&pkg).expect("读取 package.json 失败");

    // 版本号优先级：
    //   1. MENOTE_BUILD_VERSION 环境变量 —— build-release.ps1 用 -Version 指定时传入，
    //      保证 exe 文件属性与实际发布版本一致
    //   2. 回退到 package.json（直接 cargo build 时的默认值）
    let ver = match env::var("MENOTE_BUILD_VERSION") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => raw
            .split("\"version\"")
            .nth(1)
            .and_then(|s| s.split('"').nth(1))
            .expect("package.json 缺少 version 字段")
            .to_string(),
    };
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("version.rs");
    fs::write(&out, format!("pub const APP_VERSION: &str = \"{ver}\";\n"))
        .expect("写入 version.rs 失败");
    embed_icon(&manifest_dir, &ver);
    emit_window_icon_rgba(&manifest_dir);
}

/// 把窗口图标 PNG 解码为 RGBA 原始像素，写成 Rust 源码供 include! 使用。
///
/// 为什么在构建期解码：tao 的 `Icon::from_rgba` 只接受原始像素，
/// 运行时解码需把 `image` crate 打进产物（约 +300KB 且拖慢启动）。
/// 图标是固定资源，构建期解一次即可，运行时零开销。
fn emit_window_icon_rgba(manifest_dir: &str) {
    let src = Path::new(manifest_dir).join("assets").join("icon-256.png");
    println!("cargo:rerun-if-changed=assets/icon-256.png");

    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("window_icon.rs");

    if !src.exists() {
        // 缺失时降级为「无窗口图标」，不阻断构建
        println!(
            "cargo:warning=未找到 {}，窗口图标将使用系统默认。可运行 packaging/make-icon.ps1 生成。",
            src.display()
        );
        fs::write(
            &out,
            "pub const WINDOW_ICON_RGBA: Option<(&[u8], u32, u32)> = None;\n",
        )
        .expect("写入 window_icon.rs 失败");
        return;
    }

    let img = image::open(&src).expect("解码窗口图标 PNG 失败");
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let bytes = rgba.into_raw();

    // 逐字节写成数组字面量：图标约 256KB，编译期展开完全可接受
    let mut code = String::with_capacity(bytes.len() * 5 + 256);
    code.push_str(&format!(
        "/// 由 build.rs 从 assets/icon-256.png 生成（{}x{}）\n",
        w, h
    ));
    code.push_str("pub const WINDOW_ICON_RGBA: Option<(&[u8], u32, u32)> = Some((&[\n");
    for (i, b) in bytes.iter().enumerate() {
        if i % 32 == 0 {
            code.push_str("\n    ");
        }
        code.push_str(&format!("{b},"));
    }
    code.push_str(&format!("\n], {}, {}));\n", w, h));

    fs::write(&out, code).expect("写入 window_icon.rs 失败");
    println!("cargo:warning=窗口图标已内嵌（{w}x{h}, {} KB RGBA）", bytes.len() / 1024);
}

/// 嵌入 Windows 资源（图标 + 版本信息）。
///
/// 图标由 packaging/make-icon.ps1 从 fpk/ICON_256.PNG 生成，含 7 种尺寸；
/// 缺少大尺寸会让"大图标"视图模糊、任务栏在高 DPI 下发虚。
///
/// 资源编译失败不应阻断整个构建（图标属锦上添花），故仅打印警告。
fn embed_icon(manifest_dir: &str, version: &str) {
    let ico = Path::new(manifest_dir).join("assets").join("menote.ico");
    println!("cargo:rerun-if-changed=assets/menote.ico");

    if !ico.exists() {
        println!(
            "cargo:warning=未找到图标 {}，跳过嵌入。可运行 packaging/make-icon.ps1 生成。",
            ico.display()
        );
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon(ico.to_str().expect("图标路径含非 UTF-8 字符"));
    // 文件属性里显示的版本信息
    res.set("FileDescription", "MeNote 个人知识库");
    res.set("ProductName", "MeNote");
    res.set("FileVersion", version);
    res.set("ProductVersion", version);
    res.set("LegalCopyright", "MIT License");
    res.set("OriginalFilename", "MeNote.exe");
    if let Err(e) = res.compile() {
        println!("cargo:warning=嵌入图标失败（不影响功能）: {e}");
    }
}
