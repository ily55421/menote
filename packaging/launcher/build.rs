// 从项目 package.json 读取版本号，生成 APP_VERSION 常量。
// 发行版的"版本"以此为准：版本变化 → 启动器判定需要重新释放 runtime。
use std::{env, fs, path::Path};

fn main() {
    // package.json 变化时重新生成
    println!("cargo:rerun-if-changed=../../package.json");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let pkg = Path::new(&manifest_dir).join("../../package.json");
    let raw = fs::read_to_string(&pkg).expect("读取 package.json 失败");

    // 不引 serde，直接定位 "version": "x.y.z"
    let ver = raw
        .split("\"version\"")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .expect("package.json 缺少 version 字段")
        .to_string();

    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("version.rs");
    fs::write(&out, format!("pub const APP_VERSION: &str = \"{ver}\";\n"))
        .expect("写入 version.rs 失败");
}
