# RTVT - リアルタイム音声翻訳

日本語 | [中文](./README.zh.md) | [English](./README.md)

VRChat などの音声コミュニケーション向けに設計されたデスクトップ型リアルタイム音声翻訳アプリケーションです。Tauri 2（Rust + React/TypeScript）で構築され、音声認識と翻訳はすべてローカルで実行されます。外部 API は不要です。

**パイプライン**：音声キャプチャ → 音声認識（Whisper）→ 翻訳（NLLB-200）→ 音声合成 → VRChat OSC

## 特徴

- **リアルタイム音声文字起こし** — Whisper.cpp ベース、VAD（音声区間検出）内蔵
- **多言語翻訳** — NLLB-200-distilled で 200 以上の言語に対応
- **VRChat 連携** — OSC プロトコルで翻訳字幕を VRChat チャットボックスに送信
- **完全オフライン** — すべての推論をローカルで実行、ネットワーク不要
- **GPU アクセラレーション** — CUDA / Metal / OpenBLAS オプション対応
- **クロスプラットフォーム** — Windows（メイン）、macOS、Linux（一部対応）

---

## 使い方

### システム要件

| 項目 | 要件 |
|------|------|
| OS | Windows 10/11（メイン）、macOS、Linux |
| CPU | 4 コア以上推奨 |
| メモリ | 8 GB 以上 |
| ストレージ | 約 500 MB（アプリ + モデル） |
| GPU | オプション（CUDA アクセラレーション） |

### インストール

[Releases](https://github.com/MuoDoo/VRCLocalFlow/releases) ページから最新版をダウンロードし、インストーラーを実行してください。

### はじめに

1. **アプリを起動** — 初回起動時、設定パネルから必要なモデルをダウンロードしてください。
2. **オーディオデバイスを選択** — マイクまたはシステムオーディオ入力を選択します。
3. **言語を設定** — 設定画面でソース言語とターゲット言語を設定します。
4. **パイプラインを開始** — 「開始」ボタンをクリックしてリアルタイム翻訳を開始します。
5. **VRChat OSC** — 翻訳されたテキストは自動的に VRChat チャットボックスに送信されます（UDP ポート 9000）。

### モデル管理

モデルはアプリの設定パネルからオンデマンドでダウンロードできます：

| モデル | サイズ | 用途 |
|--------|--------|------|
| Whisper tiny | ~75 MB | 高速認識、精度低め |
| Whisper base | ~142 MB | バランス型（デフォルト） |
| Whisper small | ~466 MB | 高精度認識 |
| NLLB-200-distilled | ~600 MB | 翻訳（200+ 言語） |

### VRChat の設定

VRChat で OSC を有効にしてください：
1. VRChat でアクションメニューを開きます。
2. **Options → OSC → Enable** に移動します。
3. RTVT はデフォルトで `127.0.0.1:9000` にメッセージを送信します。

---

## 開発ガイド

### 前提条件

- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/)（stable、edition 2021）
- [Python 3](https://www.python.org/)（NLLB モデル変換用）
- Tauri 2 プラットフォーム依存 — [Tauri 環境構築ガイド](https://v2.tauri.app/start/prerequisites/)を参照

### クイックスタート

```bash
# リポジトリをクローン
git clone https://github.com/MuoDoo/VRCLocalFlow.git
cd VRCLocalFlow

# 初回セットアップ：依存関係インストール + モデルダウンロード
make setup

# 開発サーバーを起動
make dev
```

### ビルドコマンド

| コマンド | 説明 |
|----------|------|
| `make setup` | pnpm 依存関係インストール + 全モデルダウンロード |
| `make dev` | フルセットアップ + Tauri 開発サーバー起動 |
| `make dev-cuda` | CUDA GPU アクセラレーション付き開発モード |
| `pnpm tauri dev` | Tauri 開発起動（セットアップ済みの場合） |
| `pnpm dev` | フロントエンドのみ Vite 開発サーバー（ポート 1420） |
| `pnpm build` | フロントエントプロダクションビルド |
| `make check` | Rust 型チェック（ビルド成果物なし） |
| `make models` | Whisper + NLLB モデルダウンロード |
| `make pip-deps` | モデル変換用 Python 依存関係インストール |
| `make build-win` | Windows NSIS インストーラービルド |
| `make build-win-cuda` | CUDA 付き Windows ビルド |
| `make build-mac` | macOS .dmg ビルド（aarch64） |
| `make clean` | ダウンロード済みモデルを削除 |

### プロジェクト構成

```
VRCLocalFlow/
├── src/                        # フロントエンド（React + TypeScript）
│   ├── App.tsx                 # メインコンポーネント、パイプライン状態
│   ├── components/             # UI コンポーネント
│   │   ├── AudioSelector.tsx   # オーディオデバイス選択
│   │   ├── SettingsPanel.tsx   # 設定ドロワー
│   │   ├── StatusBar.tsx       # パイプラインステータス表示
│   │   └── SubtitleOverlay.tsx # リアルタイム字幕表示
│   └── hooks/                  # React Hooks
│       ├── useAudioDevices.ts  # オーディオデバイス列挙
│       ├── useSettings.ts      # 設定の永続化
│       └── useTranslation.ts   # ASR/翻訳イベントリスナー
├── src-tauri/                  # バックエンド（Rust + Tauri 2）
│   ├── src/
│   │   ├── lib.rs              # Tauri コマンド & 状態管理
│   │   ├── audio/              # オーディオキャプチャ & 再生（cpal）
│   │   ├── asr/                # Whisper 音声認識 + VAD
│   │   ├── translate/          # NLLB 翻訳（CTranslate2）
│   │   ├── tts/                # プラットフォーム TTS（SAPI / macOS）
│   │   ├── pipeline/           # パイプライン制御 & スレッド管理
│   │   └── vrchat/             # VRChat OSC 連携
│   └── resources/models/       # ランタイムでダウンロードされるモデル
├── Makefile                    # ビルド & セットアップコマンド
└── package.json                # フロントエンド依存関係
```

### アーキテクチャ

```
┌──────────┐    ┌───────────┐    ┌─────────────┐    ┌──────┐
│ 音声入力  │───▶│  Whisper   │───▶│   NLLB-200  │───▶│ TTS  │
│  (cpal)  │    │ （音声認識）│    │   （翻訳）   │    │      │
└──────────┘    └───────────┘    └──────┬──────┘    └──────┘
                                        │
                                 ┌──────▼──────┐
                                 │ VRChat OSC  │
                                 │（チャットボックス）│
                                 └─────────────┘
```

- **スレッドモデル**：オーディオ、ASR、翻訳はそれぞれ専用スレッドで実行され、crossbeam チャネルで接続されます。
- **プロセス間通信**：フロントエンドとバックエンドは Tauri commands（invoke）と events（emit/listen）で通信します。
- **状態管理**：バックエンドは `Mutex<T>` マネージドステート、フロントエンドは React Hooks を使用します。

### フィーチャーフラグ（Cargo Features）

| Feature | 説明 |
|---------|------|
| `cuda` | NVIDIA CUDA GPU アクセラレーション |
| `openblas` | OpenBLAS CPU アクセラレーション |
| `metal` | Apple Metal GPU アクセラレーション |
| `hipblas` | AMD HIP GPU アクセラレーション |

```bash
# 例：CUDA でビルド
make build-win-cuda
```

---

## ライセンス

[MIT](./LICENSE) © 2026 MuoDoo
