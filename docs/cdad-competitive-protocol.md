# Protocolo competitivo CDAD — preparação v1

A configuração está em `experiments/cdad_contract_protocol_v1.json`. Este
documento congela as regras de comparação; **não declara a campanha pronta**.
Gerador final de coordenadas canônicas, distribuição dos novos dados, split de
ajuste/avaliação e implementação dos baselines ainda devem ser especificados e
versionados antes das medições finais. Os seeds sozinhos não definem um dataset.

## Comparações obrigatórias

- ECEF histórico, novos dados independentes e casos de fronteira em grupos
  separados, mesma unidade e mesma codificação canônica de coordenadas.
- CBD fixo/sequencial, CDAD pelas quatro políticas e soma inteira exata das
  distâncias quadráticas. Precisões fixas 16/32/48/64, sem escolher o vencedor
  retrospectivamente sem identificá-lo como comparação exploratória.
- Média de quatro sigmoid em [-1,1], thresholds 2/5,1/2,3/5. Taylor, Padé e GL
  off-circuit; circuitos Taylor/GL só após validação de seus contratos completos.
- GL 2/4/8/16; adaptação de ordem posterior. Logistic como stress test e affine
  como controle, sem alterar seus resultados históricos.

Os dez seeds 20260908..20260917 geram mil oportunidades por aplicação cada.
Política e dados finais devem ser congelados antes de examinar o desempenho
held-out. Referência por enclosures dirigidos; inconclusivos não contam como
concordância e são reportados separadamente.

## Custo e inferência

Por oportunidade: seleção + todos os previews + witness + compromisso + todas
as provas + todas as verificações. Separar preparação de constantes e setup.
Reportar custo/oportunidade, custo/decisão certificada correta, abstenções e
cobertura. Denominador zero implica custo indefinido, nunca zero. Comparar a
mesma semântica de entrada e cobertura; ganhos apenas em constraints ficam à
parte. Declarar quais tentativas geram provas em cada política: o seletor
off-circuit atual não executa nenhuma.

Trinta rodadas pareadas e intercaladas, com ordem pré-especificada, mesmas
entradas e backend Nova 0.73/Pallas–Vesta. Intervalo de confiança pareado de
95%, respeitando agrupamento por conjunto/rodada (não tratar cada repetição de
um input como observação independente). Especificar e congelar o estimador e
procedimento de reamostragem antes da campanha. Alvo: redução >=10% no tempo
completo contra o melhor baseline elegível, com IC favorável. Resultado negativo
é válido; não alterar o protocolo para esconder vitória do baseline inteiro.

## Sobreposição científica e atribuição

| Fonte | Influência | O que não reivindicar como novidade |
|---|---|---|
| [Sridhar, USENIX 2026](https://www.usenix.org/conference/usenixsecurity26/presentation/sridhar) | Relações implícitas, erro de quantização e slack | GL ou fixed-point com bounds por si só |
| [Bitan et al., EUROCRYPT 2026](https://weizmann.elsevierpure.com/en/publications/sum-check-protocol-forapproximate-computations/) | Verificação aproximada como objeto do protocolo | Confundir tolerância numérica com probabilidade de falha criptográfica |
| [Zamir, preprint 2026](https://arxiv.org/abs/2602.15756) | Necessidade de composição adversarial | Inferir erro global só de garantias locais |
| [ZIP, CCS 2025](https://thanghoang.github.io/publication/25_ccs_zip/25_ccs_zip.pdf) | Ativações aproximadas com contratos | Alegar ser primeiro ZK de ativações aproximadas |
| [ZKLP, S&P 2025](https://discovery.ucl.ac.uk/id/eprint/10211442/) | Baseline de semântica FP e proximidade | Comparar custos publicados de backends distintos |
| [VCFloat2, CPP 2024](https://www.cs.princeton.edu/~appel/papers/vcfloat2.pdf) | Composição de justificativas de erro | Confundir rótulo analítico com prova formal |
| [Shewchuk](https://www.cs.cmu.edu/~jrs/jrspapers.html) | Predicados e precisão adaptativa | Adaptação próxima da fronteira como ideia nova |
| [Precisão mista, 2018](https://people.mpi-sws.org/~eva/papers/iccps2018.pdf), [Arb](https://arxiv.org/abs/1611.02831) | Bounds rigorosos e centro–raio | Centro–raio ou precisão mista isoladamente |

Esta matriz registra o estudo direcionado aprovado, não uma nova revisão
exaustiva. A alegação candidata é a combinação de contrato composicional para
qualquer witness aceito, vínculo canônico e eficiência de decisão no mesmo
backend. Sua originalidade e vantagem permanecem hipóteses. O manuscrito de
digital twins citado no plano continua com confirmação editorial e análise
de sobreposição pendentes; não é usado como fundamento de uma garantia.

## Gates restantes

1. Revisão independente da demonstração do checker e dos limites de domínio.
2. Vínculo vetorial canônico e teste de adulteração entre precisões em circuito.
3. Tradução das novas relações para R1CS, comparação de estados, Nova curto.
4. Baselines inteiros/CBD no mesmo backend e contabilidade completa.
5. Geradores, estatística e política final versionados; então iniciar campanha.

Não há autorização científica para alegar ganho de 10%, universalidade ZK ou
conclusão experimental de tese a partir do smoke test desta entrega.
