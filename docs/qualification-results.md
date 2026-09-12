# Resultado para a qualificação: o que uma prova numérica sustenta?

## Mensagem para o orientador

O experimento distingue provar uma computação de justificar uma decisão sobre
um sistema dinâmico. Uma prova pode certificar corretamente uma trajetória
fixed-point e, mesmo assim, a decisão dessa trajetória diferir da decisão da
dinâmica matemática pretendida. Não há contradição criptográfica: são relações
diferentes.

A proposta candidata associa a cada estado diádico um raio de erro certificado.
Só responde ao predicado quando todo o intervalo permite a mesma resposta. Caso
contrário, aumenta a precisão, partindo da mesma entrada, ou se abstém. Nesta
entrega, as tentativas são provas Nova de precisão fixa, vinculadas por uma
declaração pública comum e verificadas por um processo independente.

O resultado positivo é uma decisão acompanhada de justificativa e maior
disponibilidade em alguns horizontes. O resultado negativo é igualmente
importante: aumentar a precisão até 64 bits não elimina o limite de certificação
em caos de horizonte longo. A contribuição não é eliminar o caos nem inventar
precisão adaptativa; é conectar contrato numérico, execução verificável e
decisão, medindo também o custo dessa conexão.

## Evidência numérica

Fonte: `results/qualification_full_v1/manifest.json` e `raw.jsonl`.
Foram avaliadas 1.007 entradas e produzidos 1.782.390 registros, sem rejeições.
Mil entradas pertencem à amostra com dez seeds; a entrada histórica e seis
adversariais são analisadas separadamente. Os dados abaixo são da amostra inteira,
com r=3,9 e predicado x_N >= 1/2; cada linha contém mil oportunidades.

| Método | Horizonte | Decisões divergentes | Decisões certificadas definitivas | Abstenções |
|---|---:|---:|---:|---:|
| Fixed-point RNE, F16 | 10 | 8 | não se aplica | 0 |
| CBD local, F16 | 10 | 0 | 960 | 40 |
| CDAD sequencial | 10 | 0 | 1.000 | 0 |
| CDAD sequencial | 30 | 0 | 1.000 | 0 |
| CDAD sequencial | 100 | 0 | 11 | 989 |
| CDAD sequencial | 1.000 | 0 | 0 | 1.000 |
| Fixed-point RNE, F64 | 1.000 | 490 | não se aplica | 0 |
| FP32 restrito | 1.000 | 492 | não se aplica | 0 |

A precisão final média da CDAD foi 16,64 bits fracionários em 10 passos e 32,336
em 30 passos. O ganho de cobertura em 10 passos é de **4 pontos percentuais**
frente à precisão F16 certificada. Isso não demonstra ganho de tempo frente a
usar F64 desde o início. As três políticas adaptativas alcançam a mesma cobertura
nesta comparação; podem diferir no caminho e no custo.

Não houve decisão certificada contrária à referência conclusiva em toda a matriz.
Esse é um resultado de teste, não substituto da demonstração de inclusão. A
garantia depende das regras do contrato e da relação que o circuito implementa.

## Demonstração verificável

O pacote `results/qualification_linked_boundary_v1` contém quatro provas para a
mesma entrada pública, com dois passos: F16, F32 e F48 respondem Indeterminate;
F64 responde True. O pacote foi verificado em outro processo, sem acesso aos
objetos internos do gerador.

Além disso, `results/qualification_demonstration_v1` concluiu 104 execuções de
configuração: quatro cenários, 13 configurações, aquecimento e uma rodada. Todas
as provas foram verificadas. No caso `boundary-10`, o baseline fixed-truncate32
tem prova válida e decisão divergente da referência racional exata. Esse é um
exemplo concreto de L1 atendido sem equivalência ao predicado matemático em L3.
Não é uma quebra da soundness do Nova.

Nesse caso, a decisão provada do baseline é **False** e a referência exata é
**True**. O registro é
`results/qualification_demonstration_v1/boundary-10-round1-baseline-fixed-truncate-32-record.json`.
Para verificar novamente o pacote adaptativo, usando uma declaração autorizada
externa ao pacote e um diretório de saída novo:

```bash
python3 scripts/qualification.py verify \
  --statement results/qualification_statements_v1/boundary-2.json \
  --bundle results/qualification_linked_boundary_v1 \
  --output-dir results/qualificacao-verificacao-NOVA
```

Os testes de adulteração rejeitam mudanças em entrada, parâmetro, horizonte,
precisão, raio inicial, decisão, estado terminal e bytes de prova. As entradas
são públicas nesta entrega: não se reivindica ocultação nem vínculo privado RNE.

## Como apresentar os gráficos

1. **Trajetória:** contrastar o mesmo caso histórico em regime estável e caótico.
2. **Erro e intervalo:** distinguir erro central observado da largura do contrato.
3. **Flips e abstenções:** explicar que responder errado e não responder são eventos diferentes.
4. **Cobertura:** mostrar onde aumentar a precisão ainda permite decidir.
5. **Seleção:** explicar quais avaliações a adaptação precisou realizar desde x0.
6. **Custo real:** comparar provas curtas no mesmo backend, incluindo setup.
7. **Custo–disponibilidade:** explicitar que a medição curta não é uma fronteira populacional.

## Limites e próximos passos científicos

- A matriz longa é off-circuit; as provas completas têm 2 e 10 passos.
- O contrato de entrada é público, não uma nova construção de compromisso privado.
- Os baselines são implementações locais com semânticas documentadas; não são
  reproduções completas dos papers citados.
- A comparação com a literatura é de garantias e hipóteses, não de tempos de
  backends heterogêneos.
- A evidência não estabelece originalidade por si só. A adaptação de precisão
  orientada a predicados tem antecedentes; a alegação deve se concentrar na
  composição com contratos e provas vinculadas no problema delimitado.
- A decisão de continuar investindo em eficiência depende da campanha completa,
  não apenas de contagem de constraints ou de precisão média menor.

O protocolo, comandos de reprodução e referências primárias estão em
[qualification.md](qualification.md). Os diretórios de resultados são novos e
versionados; o código não apaga nem sobrescreve experimentos históricos.
