<#
.SYNOPSIS
Downloads the TinyLlama model for Hypercore quickstart.
#>

$ModelDir = "models"
if (!(Test-Path -Path $ModelDir)) {
    New-Item -ItemType Directory -Path $ModelDir | Out-Null
}

$ModelUrl = "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
$ModelPath = "$ModelDir\tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"

Write-Host "Downloading TinyLlama (v1.0.Q4_K_M)..."
Invoke-WebRequest -Uri $ModelUrl -OutFile $ModelPath

Write-Host "Successfully downloaded to $ModelPath" -ForegroundColor Green
Write-Host "You can now run:"
Write-Host "  hypercore serve --model $ModelPath"
