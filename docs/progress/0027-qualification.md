# 0027 — Pacote de qualificação dinâmica

Data: 2026-09-11. Entrega aditiva, preservando 0024–0026 e os hashes históricos.

## Objetivo

Transformar o experimento em uma demonstração reproduzível para a qualificação:
trajetória matemática versus representação, decisões/abstenções, CDAD com provas
vinculadas e custo medido sem comparação heterogênea de backends.

## Implementação

- Runner `qualification generate/evaluate/select`, matriz versionada, inputs
  SplitMix64-v1, referência MPFR intervalar dirigida e exportação por horizonte.
- Adaptador de trajetórias reutiliza os geradores de witness do frontier runner.
- `nova-circuit --export-proof/--verify-proof`: serialização de prova comprimida,
  declaração pública e estado terminal; reconstrução local de pp/vk no verificador.
- `scripts/qualification.py`: cenários públicos, quatro políticas, pacote vinculado,
  verificador externo, demonstração e campanha intercalada.
- Gnuplot PDF/SVG, exportação CSV, relatório e bootstrap pareado.
- `scripts/run_qualification.sh`: fluxo quick/full em diretórios novos.

## Decisões de escopo

O vínculo é `public-canonical-rne-v1`: recomputação pública exata de RNE/raio/limbs
e checagem do z0 autenticado pela prova. Não é o gadget Poseidon por truncamento,
nem um novo compromisso de entrada privada. A declaração esperada é argumento
externo obrigatório do verificador do pacote; uma declaração autoselecionada
pelo prover não é suficiente.

Logístico é a demonstração criptográfica. Afim é o controle numérico, preservando
a inicialização histórica truncada. A cobertura de CDAD fixa coincide com CBD
local e usa suas linhas na matriz; o wrapper CDAD e seu custo são medidos nas
provas. Nenhum FP32 restrito ou CFD local é chamado de reprodução fiel de um paper.

## Evidência numérica concluída

`results/qualification_full_v1`: 1.007 entradas, 1.782.390 registros,
cinco horizontes até 1.000 passos, seis configurações dinâmicas. Zero rejeições;
nenhuma decisão certificada contraditória detectada.

Na amostra independente de mil entradas, r=3,9, tau=0,5:

| Configuração | Horizonte | Flips | Definitivas certificadas | Abstenções |
|---|---:|---:|---:|---:|
| fixed-RNE F16 | 10 | 8 | não aplicável | 0 |
| CBD local F16 | 10 | 0 | 960 | 40 |
| CDAD sequencial | 10 | 0 | 1000 | 0 |
| fixed-RNE F16 | 1000 | 491 | não aplicável | 0 |
| fixed-RNE F64 | 1000 | 490 | não aplicável | 0 |
| FP32 restrito | 1000 | 492 | não aplicável | 0 |
| CBD/CDAD até F64 | 1000 | 0 | 0 | 1000 |

CDAD seleciona em média 16,64 bits fracionários no horizonte10 e 32,336 no
horizonte30, com mil decisões definitivas em ambos. No horizonte100, somente
11/1000 decisões são definitivas; no horizonte1000, nenhuma. Isso é ganho de
cobertura em horizonte curto e limite de certificação em horizonte longo,
não demonstração de superioridade universal.

## Evidência criptográfica

`results/qualification_linked_boundary_v1`: no caso público predefinido com
horizonte2, F16/F32/F48 abstêm, F64 decide True. O pacote de quatro provas foi
aceito em processo separado em `qualification_linked_boundary_verified_v1`.
Testes com adulteração de entrada, parâmetro, horizonte, precisão, raio inicial,
decisão final, estado terminal e bytes de prova rejeitaram os casos alterados.

`results/qualification_demonstration_v1` concluiu 104 execuções de configuração:
quatro cenários, 13 configurações, aquecimento e uma rodada. As provas foram
verificadas independentemente. A demonstração inclui baselines com prova válida
e decisão divergente da referência exata, além de zero contradições nas famílias
certificadas. Seus tempos são apenas demonstração, não inferência estatística.

## Verificações executadas

```
cargo test --workspace --locked --quiet
cargo clippy --workspace --locked --all-targets -- -D warnings
QUAL_BUNDLE=results/qualification_linked_boundary_v1 QUAL_STATEMENT=results/qualification_statements_v1/boundary-2.json python3 scripts/test_qualification.py
```

172 testes Rust passaram, além de 11 testes Python com provas reais habilitadas.
Inputs foram reproduzidos byte a byte. Os 1.416 registros de um smoke test foram
reproduzidos com igualdade após exclusão dos campos de tempo medido.

## Ajustes e limitações

- Corrigido no executor standard o estado inicial x0=1, que ignorava `is_one`.
- O primeiro script de gráfico tinha um erro de indexação de campos; corrigido.
  Seu diretório incompleto foi preservado, sem manifesto de conclusão.
- A matriz inteira foi executada antes de acrescentar validações adicionais de
  entrada e antes de finalizar o harness de provas. Essas validações não alteram
  a aritmética das configurações avaliadas; os hashes das fontes são preservados.
- O erro central é observado; a garantia usa o contrato. Sobreposição entre
  intervalos não é uma prova universal de inclusão.
- Tempos de demonstração executados junto de exportação/testes não sustentam
  inferência estatística. Somente a campanha isolada de 30 rodadas pode fazê-lo.
- O tempo completo inclui setup independente. A métrica com setup subtraído não
  significa que houve execução com setup reutilizado.

## Campanha de custo concluída

Em 11/09/2026 foi concluída a campanha isolada `results/qualification_timing_v1`
no cenário público `boundary-10` (r=4, horizonte 10), com aquecimento mais 30
rodadas e 13 configurações, totalizando 403 registros. Não houve falha de prova
ou verificação. A análise pareada está em `analysis.json` e usa bootstrap com
10.000 reamostragens, sem ajuste de multiplicidade.

O resultado não sustenta ganho de eficiência da adaptação frente à precisão fixa
alta: em média, `fixed F64` levou 4,5261 s por rodada e `sequential F16`
17,7887 s; `preview-all F16` levou 4,4466 s. As políticas adaptativas provaram
as decisões do cenário, enquanto F16/F32/F48 fixos se abstiveram. Portanto,
provar múltiplas tentativas custa mais neste caso. O número é específico do host,
do cenário e do backend; a conclusão científica preservada é de custo medido e
limite de eficiência, não de superioridade universal.

As figuras finais e a tabela data-driven estão em
`results/qualification_figures_final_v2`; o PDF consolidado está em
`output/pdf/qualification_v1.pdf`. Os dados brutos numéricos e de timing não são
versionados por seu tamanho, mas seus manifests, hashes e comandos de reprodução
permanecem no repositório.

## Continuidade

Ler primeiro `docs/qualification.md`. Não misturar a matriz longa off-circuit
com as provas curtas. Conferir manifests antes de declarar a campanha concluída.
Preservar os casos em que precisão fixa alta ganha da adaptação e o colapso de
cobertura no caos. Manuscrito e gráficos devem distinguir resultado observado,
contrato matemático e garantia criptográfica sobre a relação implementada.
