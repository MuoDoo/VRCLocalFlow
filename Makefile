ifeq ($(OS),Windows_NT)
SHELL := C:/Program\ Files/Git/bin/bash.exe
# MSVC needs /utf-8 for whisper.cpp Unicode source files.
# Use dash (-) prefix instead of slash (/) to prevent MSYS/Git-bash path conversion
# (e.g. /utf-8 becomes C:/Program Files/Git/utf-8 under MSYS).
# MSVC cl.exe accepts both - and / as option prefixes.
export CFLAGS=-utf-8
export CXXFLAGS=-utf-8
export CMAKE_C_FLAGS=-utf-8
export CMAKE_CXX_FLAGS=-utf-8
# Static MSVC runtime (/MT) for ALL languages including CUDA. Matches the
# rustflag in .cargo/config.toml. Keeps native deps on the same CRT so we
# never see LNK2038 mismatches. CMP0091=NEW is required for the policy
# to apply to CUDA host code via nvcc.
export CMAKE_POLICY_DEFAULT_CMP0091=NEW
export CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded
endif

MODELS_DIR := src-tauri/resources/models
WHISPER_MODEL := $(MODELS_DIR)/ggml-base.bin
NLLB_MODEL := $(MODELS_DIR)/nllb-200-distilled-600M/model.bin

.PHONY: dev setup models models-whisper models-nllb clean check build-mac build-win \
        build-engine build-engine-cpu build-engine-cuda build-engine-vulkan build-engines

# Default: setup + build engine + run
dev: setup build-engine
	pnpm tauri dev

# Install deps + download models
setup: node_modules models

node_modules: package.json pnpm-lock.yaml
	pnpm install
	@touch node_modules

# All models (whisper + NLLB)
models: models-whisper models-nllb

# Whisper model
models-whisper: $(WHISPER_MODEL)

$(WHISPER_MODEL):
	@mkdir -p $(MODELS_DIR)
	@echo "Downloading whisper ggml-base.bin (~148MB)..."
	curl -L -o $@ https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin

# NLLB translation model
models-nllb: $(NLLB_MODEL)

$(NLLB_MODEL):
	@echo "Converting facebook/nllb-200-distilled-600M → CT2 int8..."
	@mkdir -p $(MODELS_DIR)/nllb-200-distilled-600M
	ct2-transformers-converter \
		--model facebook/nllb-200-distilled-600M \
		--output_dir $(MODELS_DIR)/nllb-200-distilled-600M \
		--quantization int8 \
		--force \
		--copy_files sentencepiece.bpe.model tokenizer.json

# Install Python deps for model conversion
pip-deps:
	pip install ctranslate2 transformers sentencepiece

# ---- Engine Builds ----

# Build default engine (CPU, for dev mode)
build-engine:
	cargo build -p rtvt-engine

# Build CPU engine (release)
build-engine-cpu:
	cargo build -p rtvt-engine --release

# Build CUDA engine (release, requires CUDA toolkit)
build-engine-cuda:
	cargo build -p rtvt-engine --release --features cuda

# Build Vulkan engine (release, requires Vulkan SDK)
build-engine-vulkan:
	cargo build -p rtvt-engine --release --features vulkan

# Build all engine variants (release)
build-engines: build-engine-cpu build-engine-cuda build-engine-vulkan

# ---- Cargo Check ----

# Cargo check (main app only, no engine)
check:
	cargo check -p rtvt

# Cargo check engine
check-engine:
	cargo check -p rtvt-engine

# Clean models (re-download next time)
clean:
	rm -rf $(MODELS_DIR)/nllb-200-distilled-600M
	rm -f $(WHISPER_MODEL)

# ---- Release Build ----

# macOS: build .dmg (aarch64)
build-mac: setup build-engine
	pnpm tauri build --bundles dmg

# Windows: build NSIS installer (run on Windows natively)
build-win: setup
	pnpm tauri build --bundles nsis

# Dev mode with pre-built engine
dev-engine: setup
	pnpm tauri dev
