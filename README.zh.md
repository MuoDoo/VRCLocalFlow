# RTVT - 实时语音翻译

日本語](./README.ja.md) | 中文 | [English](./README.md)

一款桌面端实时语音翻译应用，专为 VRChat 等语音交流场景设计。基于 Tauri 2（Rust + React/TypeScript）构建，所有语音识别和翻译均在本地运行，无需任何外部 API。

**处理流程**：音频采集 → 语音识别（Whisper）→ 翻译（NLLB-200）→ 语音合成 → VRChat OSC

## 特性

- **实时语音转写** — 基于 Whisper.cpp，内置 VAD 语音活动检测
- **多语言翻译** — NLLB-200-distilled（当前界面已开放英 / 中 / 日；底层模型可通过少量数据驱动改动扩展更多语言）
- **VRChat 集成** — 通过 OSC 协议将翻译字幕发送到 VRChat 聊天框
- **完全离线** — 所有推理在本地完成，无需网络
- **GPU 加速** — 可选 CUDA / Metal / OpenBLAS 支持
- **跨平台** — Windows（主要）、macOS、Linux（部分支持）

---

## 使用说明

### 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Windows 10/11（主要）、macOS、Linux |
| CPU | 推荐 4 核以上 |
| 内存 | 8 GB 以上 |
| 存储空间 | 约 500 MB（应用 + 模型） |
| GPU | 可选（CUDA 加速） |

### 安装

从 [Releases](https://github.com/MuoDoo/VRCLocalFlow/releases) 页面下载最新版本的安装包并运行。

### 快速上手

1. **启动应用** — 首次启动时，在设置面板中下载所需模型。
2. **选择音频设备** — 选择麦克风或系统音频输入。
3. **设置语言** — 在设置中选择源语言和目标语言。
4. **开始翻译** — 点击「开始」按钮启动实时翻译管线。
5. **VRChat OSC** — 翻译后的文本会自动发送到 VRChat 聊天框（UDP 端口 9000）。

### 模型管理

模型可通过应用设置面板按需下载：

| 模型 | 大小 | 用途 |
|------|------|------|
| Whisper tiny | ~75 MB | 快速识别，精度较低 |
| Whisper base | ~142 MB | 均衡（默认） |
| Whisper small | ~466 MB | 高精度识别 |
| NLLB-200-distilled | ~600 MB | 翻译（界面开放 en/zh/ja，模型本身支持更多语言） |

### VRChat 设置

确保 VRChat 已启用 OSC：
1. 在 VRChat 中打开动作菜单（Action Menu）。
2. 进入 **Options → OSC → Enable**。
3. RTVT 默认发送消息到 `127.0.0.1:9000`。

---

## 开发指南

### 前置条件

- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/)（stable，edition 2021）
- [Python 3](https://www.python.org/)（用于 NLLB 模型转换）
- Tauri 2 平台依赖 — 参见 [Tauri 环境配置](https://v2.tauri.app/start/prerequisites/)

### 快速开始

```bash
# 克隆仓库
git clone https://github.com/MuoDoo/VRCLocalFlow.git
cd VRCLocalFlow

# 首次配置：安装依赖 + 下载模型
make setup

# 启动开发服务器
make dev
```

### 构建命令

| 命令 | 说明 |
|------|------|
| `make setup` | 安装 pnpm 依赖 + 下载所有模型 |
| `make dev` | 完整配置 + 启动 Tauri 开发服务器 |
| `make dev-cuda` | 启用 CUDA GPU 加速的开发模式 |
| `pnpm tauri dev` | 启动 Tauri 开发（需已完成 setup） |
| `pnpm dev` | 仅前端 Vite 开发服务器（端口 1420） |
| `pnpm build` | 前端生产构建 |
| `make check` | Rust 类型检查（不生成构建产物） |
| `make models` | 下载 Whisper + NLLB 模型 |
| `make pip-deps` | 安装模型转换所需的 Python 依赖 |
| `make build-win` | 构建 Windows NSIS 安装包 |
| `make build-win-cuda` | 构建带 CUDA 的 Windows 安装包 |
| `make build-mac` | 构建 macOS .dmg（aarch64） |
| `make clean` | 删除已下载的模型 |

### 项目结构

```
VRCLocalFlow/
├── src/                        # 前端（React + TypeScript）
│   ├── App.tsx                 # 主组件，管线状态管理
│   ├── components/             # UI 组件
│   │   ├── AudioSelector.tsx   # 音频设备选择
│   │   ├── SettingsPanel.tsx   # 设置抽屉
│   │   ├── StatusBar.tsx       # 管线状态栏
│   │   └── SubtitleOverlay.tsx # 实时字幕显示
│   └── hooks/                  # React Hooks
│       ├── useAudioDevices.ts  # 音频设备枚举
│       ├── useSettings.ts      # 设置持久化
│       └── useTranslation.ts   # ASR/翻译事件监听
├── src-tauri/                  # 后端（Rust + Tauri 2）
│   ├── src/
│   │   ├── lib.rs              # Tauri 命令与状态管理
│   │   ├── audio/              # 音频采集与播放（cpal）
│   │   ├── asr/                # Whisper 语音识别 + VAD
│   │   ├── translate/          # NLLB 翻译（CTranslate2）
│   │   ├── tts/                # 平台 TTS（SAPI / macOS）
│   │   ├── pipeline/           # 管线编排与线程管理
│   │   └── vrchat/             # VRChat OSC 集成
│   └── resources/models/       # 运行时下载的模型
├── Makefile                    # 构建与配置命令
└── package.json                # 前端依赖
```

### 架构

```
┌──────────┐    ┌───────────┐    ┌─────────────┐    ┌──────┐
│ 音频输入  │───▶│  Whisper   │───▶│   NLLB-200  │───▶│ TTS  │
│  (cpal)  │    │ （语音识别）│    │  （翻译）    │    │      │
└──────────┘    └───────────┘    └──────┬──────┘    └──────┘
                                        │
                                 ┌──────▼──────┐
                                 │ VRChat OSC  │
                                 │  （聊天框）  │
                                 └─────────────┘
```

- **线程模型**：音频、ASR、翻译各运行在独立线程上，通过 crossbeam channel 通信。
- **进程间通信**：前后端通过 Tauri commands（invoke）和 events（emit/listen）通信。
- **状态管理**：后端使用 `Mutex<T>` 托管状态；前端使用 React Hooks。

### 功能标志（Cargo Features）

| Feature | 说明 |
|---------|------|
| `cuda` | NVIDIA CUDA GPU 加速 |
| `openblas` | OpenBLAS CPU 加速 |
| `metal` | Apple Metal GPU 加速 |
| `hipblas` | AMD HIP GPU 加速 |

```bash
# 示例：使用 CUDA 构建
make build-win-cuda
```

---

## 许可证

[MIT](./LICENSE) © 2026 MuoDoo
