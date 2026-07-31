# Modelo local de custo FP32

Este artefato não é um benchmark de gadget IEEE 754 completo. Ele é um modelo
local, reprodutível, para o subconjunto FP32 positivo e normal no mapa
logístico:

```text
a = r*x
b = 1 - x
y = a*b
```

O objetivo é evitar uma comparação verbal vaga como “IEEE é caro” sem, ao
mesmo tempo, afirmar que medimos um circuito FP32 bit-exato.

O CSV gerado é:

```text
results/fp32_constraint_model.csv
```

## Escopo incluído

- duas multiplicações FP32;
- uma subtração FP32;
- decomposição de valores FP32 em bits;
- classificação positiva-normal mínima;
- aritmética de expoente;
- normalização;
- round-to-nearest-even por guard/round/sticky;
- duas estratégias para multiplicação de mantissas:
  - `packed_integer_optimistic`;
  - `bit_level_schoolbook`.

## Escopo excluído

- NaN;
- infinito;
- subnormal;
- sinais gerais;
- overflow e underflow completos;
- todos os caminhos de exceção IEEE;
- prova de equivalência com uma biblioteca IEEE 754;
- integração com Nova.

## Interpretação

A linha `packed_integer_optimistic` é uma estimativa agressivamente favorável
ao FP32: trata multiplicações de mantissa como relações inteiras compactas e
mantém apenas buckets mínimos de decomposição, normalização e arredondamento.

A linha `bit_level_schoolbook` representa uma implementação bit-level direta da
multiplicação de mantissas, com `24×24` produtos bit-a-bit por multiplicação
FP32. Ela não é otimizada, mas explicita o custo que aparece quando se exige
aritmética bit-exata sem usar um gadget especializado.

Nenhuma das duas linhas deve ser apresentada como custo medido homogêneo contra
os circuitos em limbs. Elas são limites/modelos locais para orientar a próxima
implementação de baseline.
