ifeq ($(OS),Windows_NT)
SHELL := C:/Program\ Files/Git/bin/bash.exe
# MSVC needs /utf-8 for whisper.cpp Unicode source files.
# Use dash (-) prefix instead of slash (/) to prevent MSYS/Git-bash path conversion
# (e.g. /utf-8 becomes C:/Program Files/Git/utf-8 under MSYS).
# MSVC cl.exe accepts both - and / as option prefixes.
# CMAKE_*_RELEASE must include full MSVC defaults (-MT -O2 -Ob2 -DNDEBUG)
# because setting them replaces cmake's defaults rather than appending.
export CFLAGS=-utf-8
export CXXFLAGS=-utf-8
export CMAKE_C_FLAGS=-utf-8
export CMAKE_CXX_FLAGS=-utf-8
export CMAKE_C_FLAGS_RELEASE=-MT -O2 -Ob2 -DNDEBUG -utf-8
export CMAKE_CXX_FLAGS_RELEASE=-MT -O2 -Ob2 -DNDEBUG -utf-8
endif

MODELS_DIR := src-tauri/resources/models
WHISPER_MODEL := $(MODELS_DIR)/ggml-base.bin
NLLB_MODEL := $(MODELS_DIR)/nllb-200-distilled-600M/model.bin

.PHONY: dev setup models models-whisper models-nllb clean check build-mac build-win build-win-vulkan dev-vulkan check-vulkan

# Default: setup + run
dev: setup
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

# Cargo check only
check:
	cd src-tauri && cargo check

# Cargo check with CUDA GPU acceleration
check-cuda:
	cd src-tauri && cargo check --features cuda

# Cargo check with Vulkan GPU acceleration (Nvidia + AMD)
check-vulkan:
	cd src-tauri && cargo check --features vulkan

# Clean models (re-download next time)
clean:
	rm -rf $(MODELS_DIR)/nllb-200-distilled-600M
	rm -f $(WHISPER_MODEL)

# ---- Release Build ----

# macOS: build .dmg (aarch64)
build-mac: setup
	pnpm tauri build --bundles dmg

# Windows: build NSIS installer (run on Windows natively)
build-win: setup
	pnpm tauri build --bundles nsis

# Windows: build with CUDA GPU acceleration (requires CUDA toolkit)
build-win-cuda: setup
	TAURI_CARGO_FLAGS="--features cuda" pnpm tauri build --bundles nsis

# Windows: build with Vulkan GPU acceleration (requires Vulkan SDK, works with Nvidia + AMD)
build-win-vulkan: setup
	TAURI_CARGO_FLAGS="--features vulkan" pnpm tauri build --bundles nsis

# Dev mode with CUDA GPU acceleration
dev-cuda: setup
	TAURI_CARGO_FLAGS="--features cuda" pnpm tauri dev

# Dev mode with Vulkan GPU acceleration (Nvidia + AMD)
dev-vulkan: setup
	TAURI_CARGO_FLAGS="--features vulkan" pnpm tauri dev
