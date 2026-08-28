#!/bin/sh
# Sample component demonstrating real per-machine model selection via
# `mlai catalog resolve`, driven by the bundled catalog/trusted-autonomy.toml
# fragment. Requires the `mlai` CLI to be on PATH (the same assumption any
# real adopter's setup script makes -- see samples/README.md). If `mlai`
# isn't found, `--describe-options` fails naturally and mlai-gui/mlai-cli
# both already treat that as "no options for this component," not an error.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CATALOG="$SCRIPT_DIR/catalog/trusted-autonomy.toml"

# Illustrative hardware profile -- real hardware auto-detection is explicitly
# out of scope for this project (see docs/superpowers/specs/2026-08-15-
# distribution-packaging-framework-design.md); a real adopter's setup script
# would substitute its own detection here. Defaults vary by host OS so the
# demo resolves a different, OS-appropriate tier without configuration;
# override via env vars to see other tiers resolve.
HOST_OS="$(uname -s)"
case "$HOST_OS" in
  Darwin) DEFAULT_OS="macos"; DEFAULT_VENDOR="apple"; DEFAULT_VRAM=0 ;;
  *) DEFAULT_OS="linux"; DEFAULT_VENDOR="nvidia"; DEFAULT_VRAM=8 ;;
esac
MLAI_SAMPLE_OS="${MLAI_SAMPLE_OS:-$DEFAULT_OS}"
MLAI_SAMPLE_GPU_VENDOR="${MLAI_SAMPLE_GPU_VENDOR:-$DEFAULT_VENDOR}"
MLAI_SAMPLE_VRAM_GB="${MLAI_SAMPLE_VRAM_GB:-$DEFAULT_VRAM}"

if [ "${1:-}" = "--describe-options" ]; then
  RESOLVED="$(mlai catalog resolve \
    --purpose voice-transcription \
    --catalog "$CATALOG" \
    --os "$MLAI_SAMPLE_OS" \
    --gpu-vendor "$MLAI_SAMPLE_GPU_VENDOR" \
    --vram-gb "$MLAI_SAMPLE_VRAM_GB" \
    --effective-vram-gb "$MLAI_SAMPLE_VRAM_GB" \
    --disk-free-gb 50)"
  printf '{"schema_version":1,"options":[{"key":"model","label":"Transcription model (resolved for this machine)","type":"choice","choices":[{"value":"%s","label":"%s","recommended":true}],"default":"%s"}]}\n' \
    "$RESOLVED" "$RESOLVED" "$RESOLVED"
  exit 0
fi

MODEL="unset"
while [ $# -gt 0 ]; do
  case "$1" in
    --set)
      shift
      case "$1" in
        model=*) MODEL="${1#model=}" ;;
      esac
      ;;
  esac
  shift
done

echo "selected model: $MODEL" > selected-model.txt
touch marker.txt
