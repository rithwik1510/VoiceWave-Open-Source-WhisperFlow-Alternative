#!/usr/bin/env bash
set -euo pipefail

required_files=(
  "README.md"
  "CONTRIBUTING.md"
  "docs/prd/v1-prd.md"
  "docs/phase0-signoff.md"
  "docs/rfc/0001-system-architecture.md"
  "docs/testing/test-strategy.md"
  "docs/testing/hardware-tiers.md"
  "docs/benchmarks/competitive-benchmark-v1.md"
  "docs/risk/risk-register.md"
  "docs/security/threat-model-v1.md"
  "docs/adr/README.md"
  "docs/adr/template.md"
  "docs/adr/0001-phase-0-locked-decisions.md"
  ".github/pull_request_template.md"
  ".github/workflows/ci.yml"
  ".markdownlint-cli2.yaml"
  ".prettierignore"
  ".gitignore"
)

missing=()
for file in "${required_files[@]}"; do
  if [[ ! -f "$file" ]]; then
    missing+=("$file")
  fi
done

if [[ "${#missing[@]}" -gt 0 ]]; then
  echo "Missing required Phase 0 files:"
  for file in "${missing[@]}"; do
    echo " - $file"
  done
  exit 1
fi

grep -q "^# VoiceWave v1 PRD$" "docs/prd/v1-prd.md"
grep -q "^# RFC 0001: VoiceWave v1 System Architecture$" "docs/rfc/0001-system-architecture.md"
grep -q "^# VoiceWave Test Strategy$" "docs/testing/test-strategy.md"
grep -q "^## Decision$" "docs/adr/0001-phase-0-locked-decisions.md"

if [[ -f "package.json" ]]; then
  if [[ ! -f "package-lock.json" ]]; then
    echo "package.json exists but package-lock.json is missing"
    exit 1
  fi
  grep -q '"dev"' "package.json"
  grep -q '"build"' "package.json"
  grep -q '"test"' "package.json"
fi

if [[ -f "src-tauri/Cargo.toml" ]]; then
  grep -q "^\[package\]" "src-tauri/Cargo.toml"
fi

echo "Phase 0 artifact verification passed."
