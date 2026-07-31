# 0007 — Referência FP32 restrita

**Data:** 2026-07-05  
**Estado:** parcial; referência validada, circuito exato pendente

## Objetivo

Construir o baseline FP32 positivo-normal e validá-lo contra implementação
independente antes de escrever o gadget R1CS.

## Arquivos modificados

- `crates/numeric-reference/src/fp32.rs`
- `crates/numeric-reference/src/lib.rs`
- `crates/numeric-reference/Cargo.toml`
- `tools/softfloat-validation/`
- `.cargo/config.toml`
- `Cargo.toml`
- `Cargo.lock`

## Implementação

`RestrictedFp32`:

- decodifica bits, expoente e significando;
- aceita zero positivo e normais positivos;
- rejeita sinal, subnormal, NaN e infinito;
- executa `1-x`, `x(1-x)` e multiplicação por `r` em operações separadas;
- preserva os bits de cada intermediário.

## Testes

Dez mil entradas normais pseudoaleatórias foram comparadas com o crate
software `softfloat 1.0.0`. Complemento, produto e saída coincidiram bit a bit.

## Tentativas malsucedidas

1. `softfloat-wrapper` falhou inicialmente porque seu `c99` declara uma versão
   antiga de CMake. A política mínima foi configurada.
2. A build seguinte falhou porque `softfloat-sys 0.1.4` não implementa regras
   para `aarch64-apple-darwin`.
3. A compilação cruzada x86_64 falhou porque `gmp-mpfr-sys`, dependência da
   referência principal, não aceita esse cross-build.
4. Um crate isolado removeu GMP do caminho, mas o build script de
   `softfloat-sys` usa a arquitetura do host e ainda rejeita Apple Silicon.

O teste Berkeley foi preservado em `tools/softfloat-validation` para execução
em host x86_64 real.

## Limitações conhecidas

- Ainda não existe gadget FP32 R1CS exato.
- A validação ativa em Apple Silicon usa uma implementação software
  independente, mas não Berkeley SoftFloat.
- Portanto a hipótese de menor custo que FP32 permanece não demonstrada.

## Próximo passo

Implementar subtração, multiplicação, normalização e nearest-even em R1CS ou
executar o gadget em um host x86_64 que também permita a validação Berkeley.

## Continuidade para outra IA

Não transforme `fp32_constraint_model.csv` em benchmark medido. O bloqueio é
o gadget circuital, não a referência de bits.
