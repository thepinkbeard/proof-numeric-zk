# 0010 — Gadgets FP32 restritos exatos

**Data:** 2026-07-05  
**Estado:** concluído

## Objetivo

Implementar o subconjunto FP32 positivo-normal no mesmo R1CS/Nova do CBD, sem
FMA ou lookups, provando normalização e round-to-nearest-ties-to-even.

## Arquivos modificados

- `crates/nova-circuit/src/fp32.rs`
- `crates/nova-circuit/src/lib.rs`
- `crates/nova-circuit/Cargo.toml`
- `Cargo.lock`
- `docs/fp32-r1cs.md`

## Decisões e hipóteses

- `CircuitFp32` representa `+0` canônico ou um normal positivo por expoente
  enviesado, significando explícito de 24 bits e bit de zero.
- Negativos não têm representação; expoente zero com significando não nulo,
  expoente 255, underflow e overflow são rejeitados.
- Multiplicação prova produto de 48 bits, escolha de normalização, quociente,
  resto, RNE e carry.
- `1-x` usa escala inteira global `2^-149` e prova potências de dois
  selecionadas pelos bits do expoente, evitando 126 ramos.
- A forma R1CS é independente do witness, inclusive no caso divisor igual a
  um.

## Comandos executados

```bash
cargo check -p nova-circuit --all-targets
cargo test -p nova-circuit fp32::tests
```

## Testes, métricas e evidências

- Gadgets públicos: `Fp32MulCircuit`, `Fp32SubOneCircuit` e
  `RestrictedFp32StepCircuit`.
- Casos normais, zero, `x=1`, expoentes distantes e transições de
  normalização são satisfeitos.
- Witnesses com saída, arredondamento, decisão ou significando adulterados são
  rejeitados.
- A etapa completa expõe cinco elementos de estado: contador, expoente,
  significando, zero e decisão.

## Tentativas malsucedidas

A primeira regra de RNE adicionava uma constraint condicional quando o divisor
era um. Isso faria a forma circuital depender do witness. A regra foi
substituída por um indicador algébrico de `divisor=1` e seleção uniforme.

## Limitações conhecidas

- O perfil não cobre sinal geral, subnormais, NaN ou infinito.
- A etapa usa parâmetro público fixo e domínio `x∈[0,1]`, `r∈[0,4]`.
- Ainda não há benchmark Nova neste registro.

## Próximo passo

Validar diferencialmente, cobrir fronteiras de RNE e tentar Berkeley
SoftFloat/TestFloat em x86_64.

## Continuidade para outra IA

Não transforme `CircuitFp32` em bits IEEE empacotados sem preservar os ranges e
a canonicalidade já provados em `fp32.rs`.
