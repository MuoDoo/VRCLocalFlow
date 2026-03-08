#!/bin/bash
# Download and convert Opus-MT models for en↔zh translation.
# Requires: pip install ctranslate2 transformers sentencepiece
#
# Output directories:
#   src-tauri/resources/models/opus-mt-en-zh/
#   src-tauri/resources/models/opus-mt-zh-en/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MODELS_DIR="$PROJECT_ROOT/src-tauri/resources/models"

convert_model() {
    local hf_model="$1"
    local output_dir="$2"

    if [ -f "$output_dir/model.bin" ]; then
        echo "Model already exists at $output_dir, skipping."
        return
    fi

    echo "Converting $hf_model → $output_dir ..."
    ct2-transformers-converter \
        --model "$hf_model" \
        --output_dir "$output_dir" \
        --quantization int8 \
        --copy_files source.spm target.spm

    echo "Done: $output_dir"
}

echo "=== Downloading & converting Opus-MT models ==="
echo ""

convert_model "Helsinki-NLP/opus-mt-en-zh" "$MODELS_DIR/opus-mt-en-zh"
echo ""
convert_model "Helsinki-NLP/opus-mt-zh-en" "$MODELS_DIR/opus-mt-zh-en"

echo ""
echo "=== All models ready ==="
echo "Model files:"
ls -lh "$MODELS_DIR/opus-mt-en-zh/"
echo ""
ls -lh "$MODELS_DIR/opus-mt-zh-en/"
