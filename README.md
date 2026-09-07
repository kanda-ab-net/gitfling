# GitFling

git-ftp のGUIツール。ローカルのGit差分を確認し、リモートサーバー（FTP/FTPS/SFTP）へアップロード（デプロイ）します。Apple 風の Liquid Glass デザイン。

- **フロントエンド**: Vite + Vanilla JS（Liquid Glass CSS）
- **バックエンド**: Rust + Tauri v2
- **Git差分**: `git2`（libgit2）
- **転送**: `suppaftp`（FTP/FTPS）/ `ssh2`（SFTP）
- **追跡方式**: git-ftp互換。サーバー上の `.git-ftp.log` に最終デプロイのコミットハッシュを記録

## 仕組み

1. サーバー上の `<remote_root>/.git-ftp.log` から前回デプロイしたコミットハッシュを読む
2. そのコミットから現在の `HEAD` までの差分（追加/変更/削除）を計算して一覧表示
3. 選択したファイルの**作業ツリーの内容**をアップロード、削除ファイルはリモートから削除
4. 完了後、`.git-ftp.log` を現在の `HEAD` のハッシュで更新

> 初回（`.git-ftp.log` が無い）時は、`HEAD` の全追跡ファイルが「追加」として表示されます。

## インストール（Homebrew）

```bash
brew tap kanda-ab-net/gitfling https://github.com/kanda-ab-net/gitfling
brew install --cask gitfling
```

- 対応: Apple Silicon (arm64) macOS のみ
- 署名・公証（notarization）は行っていないため、初回起動時に Gatekeeper の警告が出ます。
  「システム設定 > プライバシーとセキュリティ」から「このまま開く」を選択するか、
  ターミナルで `xattr -dr com.apple.quarantine /Applications/GitFling.app` を実行してください。

新しいリリースの出し方は [`.github/workflows/release.yml`](.github/workflows/release.yml) を参照。
`git tag vX.Y.Z && git push origin vX.Y.Z` で GitHub Actions が `.dmg` をビルドし、下書きリリースを作成します。
公開後、[`Casks/gitfling.rb`](Casks/gitfling.rb) の `version` と `sha256`（`shasum -a 256 GitFling_X.Y.Z_aarch64.dmg`）を更新してください。

## 必要なもの（開発環境）

- Node.js / npm
- Rust（stable, rustup）
- macOS + Xcode Command Line Tools
- OpenSSL（`ssh2` 用）: `brew install openssl@3`

## セットアップ

```bash
npm install
```

## 開発起動

```bash
# OpenSSL の場所を Rust に伝える（ssh2 のビルドに必要）
export OPENSSL_DIR="$(brew --prefix openssl@3)"
export PKG_CONFIG_PATH="$OPENSSL_DIR/lib/pkgconfig"

npm run tauri dev
```

## リリースビルド（.app / .dmg）

```bash
export OPENSSL_DIR="$(brew --prefix openssl@3)"
export PKG_CONFIG_PATH="$OPENSSL_DIR/lib/pkgconfig"

npm run tauri build
```

生成物は `src-tauri/target/release/bundle/` に出力されます。

## 使い方

1. 右上の **＋** でサーバープロファイルを作成
   （プロトコル/ホスト/ポート/ユーザー/パスワード/ローカルGitリポジトリ/ローカルの同期ベースフォルダ/リモートルート）
2. **🔌 接続** でサーバーに接続（リモートのファイル階層がツリー表示される）
3. 左ペインでアップロードするファイルにチェック
4. **⇧ アップロード** でデプロイ

## デプロイ除外（.git-ftp-ignore）

リポジトリルートに `.git-ftp-ignore` を置くと、**Gitでは管理するがサーバーには上げたくない**ファイルを除外できます（`.gitignore` 対象はそもそも追跡外なので最初からアップロードされません）。

- 書式は **`.gitignore` 風の glob**（1行1パターン、`#` でコメント、`!` で除外解除）
- 除外されたファイルは差分一覧から隠れ、init（全アップロード）でも対象外になります
- ステータスバーに除外件数が表示されます

例:

```
# ソースマップは上げない
*.map
# ソースディレクトリごと除外
src/**
# ドキュメント
README.md
# 依存ディレクトリ
node_modules/
```

## 注意

- プロファイル（パスワード含む）はこの端末の設定ディレクトリ
  `~/Library/Application Support/gitfling/profiles.json` に平文で保存されます。
- アップロードされるのは「作業ツリー上の実ファイル」です（コミット済み内容ではなく、
  ディスク上の現在の内容）。差分の判定は `.git-ftp.log` のコミット〜`HEAD` 間で行います。
