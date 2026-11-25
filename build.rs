//! Windows 向けでは exe にアイコン（`assets/vzeditor.ico`）を埋め込む。
//! ビルド日時 `VZ_BUILD_AT` は About を描く vz-ui の build.rs が埋め込む。

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/vzeditor.ico");

    // ホストではなくビルド対象の OS で判定する（Windows 上の Linux 向け check では埋め込まない）
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/vzeditor.ico");
        res.compile().expect("exe へのアイコン埋め込みに失敗");
    }
}
