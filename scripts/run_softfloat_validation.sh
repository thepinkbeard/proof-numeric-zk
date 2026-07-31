#!/usr/bin/env bash
# Validação diferencial contra Berkeley SoftFloat em host x86_64 Linux.
#
# `softfloat-sys` decide as regras de build por `cfg!(all(target_arch = "x86_64",
# target_os = "linux"))`, que num build script avalia o **host**. Isso significa
# que nem Rosetta nem `--target x86_64-apple-darwin` resolvem em Apple Silicon:
# não é questão de alvo, é questão de onde o build script roda. Foi o que travou
# a rodada 0007.
#
# Este script executa a validação num host x86_64 Linux emulado. `m4` é
# dependência de `gmp-mpfr-sys`, e `cmake` do `c99` transitivo de SoftFloat.
# A emulação torna a compilação lenta — cerca de oito minutos — mas os testes
# em si rodam em milissegundos.
#
# Em CI x86_64 nativo, use `.github/workflows/softfloat.yml`, que dispensa a
# emulação e faz a mesma coisa.
set -euo pipefail

cd "$(dirname "$0")/.."

exec docker run --rm --platform linux/amd64 \
  -v "$PWD":/work -w /work \
  rust:1.85-bookworm \
  bash -c '
    set -e
    apt-get update -qq >/dev/null
    apt-get install -y -qq m4 cmake >/dev/null
    export CARGO_TARGET_DIR=/tmp/target
    cargo test -p softfloat-validation --release
  '
