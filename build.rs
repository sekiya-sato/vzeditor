//! ビルド日時を `VZ_BUILD_AT`（例: 2026/11/24 12:00、ローカル時刻）として埋め込む。
//! ソース・Cargo.toml・build.rs が変わってビルドし直したときに更新される。
//! Windows 向けでは exe にアイコン（`assets/vzeditor.ico`）を埋め込む。

fn main() {
    let now = chrono::Local::now();
    println!("cargo:rustc-env=VZ_BUILD_AT={}", now.format("%Y/%m/%d %H:%M"));
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/vzeditor.ico");

    // ホストではなくビルド対象の OS で判定する（Windows 上の Linux 向け check では埋め込まない）
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/vzeditor.ico");
        res.compile().expect("exe へのアイコン埋め込みに失敗");
    }
}
