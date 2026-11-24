//! ビルド日時を `VZ_BUILD_AT`（例: 2026/11/24 12:00、ローカル時刻）として埋め込む。
//! ソース・Cargo.toml・build.rs が変わってビルドし直したときに更新される。

fn main() {
    let now = chrono::Local::now();
    println!("cargo:rustc-env=VZ_BUILD_AT={}", now.format("%Y/%m/%d %H:%M"));
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
}
