#!/usr/bin/env bash
set -e

mkdir -p models
MODEL_URL="https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
MODEL_PATH="models/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"

echo "Downloading TinyLlama (v1.0.Q4_K_M)..."
if command -v curl &> /dev/null; then
    curl -L "$MODEL_URL" -o "$MODEL_PATH"
elif command -v wget &> /dev/null; then
    wget -O "$MODEL_PATH" "$MODEL_URL"
else
    echo "Error: Neither curl nor wget was found."
    exit 1
fi

echo "Successfully downloaded to $MODEL_PATH"
echo "You can now run:"
echo "  hypercore serve --model $MODEL_PATH"
