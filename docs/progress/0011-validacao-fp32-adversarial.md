# 0011 — Validação diferencial e adversarial FP32

**Data:** 2026-07-05  
**Estado:** concluído com validação Berkeley pendente

## Objetivo

Demonstrar que as testemunhas FP32 coincidem bit a bit com a referência e que
os circuitos rejeitam atalhos de normalização, arredondamento e decisão.

## Arquivos modificados

- `crates/nova-circuit/src/fp32.rs`
- `tools/softfloat-validation/src/lib.rs` (preservado, sem alteração)

## Decisões e hipóteses

- A referência ativa é `RestrictedFp32`, já comparada com o crate independente
  `softfloat`.
- Dez mil vetores exercitam a construção da testemunha; os primeiros 256
  também sintetizam e verificam o R1CS para manter o teste rápido.
- Casos construídos cobrem empate que arredonda para cima, empate que permanece
  par e carry que renormaliza o resultado.

## Comandos executados

```bash
cargo test -p nova-circuit fp32::tests
cargo test -p numeric-reference native_trace_matches_independent_software_float
cargo test -p softfloat-validation --target x86_64-apple-darwin
docker info --format '{{.Architecture}} {{.OSType}}'
```

## Testes, métricas e evidências

- 10.000/10.000 saídas coincidiram bit a bit com a referência.
- 256/256 testemunhas pseudoaleatórias produziram R1CS satisfeito.
- A mesma quantidade de constraints foi observada para `x=0`, `0,1`, `0,25`,
  `0,5`, `0,9` e `1`, confirmando forma independente do witness.
- Arredondamento, saída, decisão e codificação não canônica adulterados foram
  rejeitados.

## Tentativas malsucedidas

- `softfloat-sys 0.1.4` falhou novamente no cross-build
  `x86_64-apple-darwin`; seu build script decide pelas características do host
  arm64 e informa que não há regras para o alvo.
- O executável Docker existe, mas o daemon configurado pelo OrbStack não está
  disponível. Portanto não foi possível executar a validação em contêiner
  `linux/amd64`.

## Limitações conhecidas

- Berkeley SoftFloat/TestFloat ainda não foi executado.
- A concordância ativa usa uma segunda implementação software, não a suíte
  oficial Berkeley.
- Os 10.000 vetores pertencem ao domínio positivo-normal do workload.

## Próximo passo

Executar `cargo test -p softfloat-validation` em host x86_64 real e anexar o
resultado em um novo registro, sem alterar este arquivo.

## Continuidade para outra IA

O bloqueio Berkeley é ambiental. Não reabra o gadget nem marque essa validação
como concluída sem saída real de um host x86_64.
