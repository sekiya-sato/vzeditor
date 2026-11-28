# Screenshots Guide / スクリーンショット撮影ガイド

`README.md` では以下の2枚の画像を参照しています。

- `docs/screenshots/editor.png` — エディタ画面（複数ファイルを開いてバッファ一覧バーや日本語表示が見える状態がおすすめ）
- `docs/screenshots/filer.png` — 2画面ファイラー画面（`ESC` で切替後、`Space` で1〜2ファイルマークした状態がおすすめ）

## 撮影手順（おすすめ：GNOME 標準スクリーンショット）

1. サンプルファイル付きで VZ Editor を起動します:
   ```bash
   cargo run --locked --bin vz -- examples/welcome_ja.txt
   ```
2. （任意）`ESC` でファイラーを開き、`Cargo.toml` などを `Enter` で開くと、上部に複数バッファ切り替えバー（`^W バッファ:`）が表示されて見栄えが良くなります。
3. キーボードの `Print`（PrtSc）キーを押し、「ウィンドウ」モードで VZ Editor ウィンドウを撮影します。
4. 撮影した画像を以下に保存・配置します:
   - エディタ画面 → `docs/screenshots/editor.png`
   - ファイラー画面（`ESC` 押下後）→ `docs/screenshots/filer.png`

## スクリプトから撮影する場合

プロジェクトルートから補助スクリプトを実行することもできます（3秒後にアクティブウィンドウをキャプチャします）:

```bash
./scripts/take_screenshot.sh editor   # -> docs/screenshots/editor.png
./scripts/take_screenshot.sh filer    # -> docs/screenshots/filer.png
```
