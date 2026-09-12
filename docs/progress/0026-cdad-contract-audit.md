# 0026 — Auditoria e contratos composicionais CDAD

Data: 2026-09-08. Estado: marco off-circuit implementado; campanha e novos
circuitos pendentes. Continuação aditiva de 0024/0025, sem substituí-los.

## Objetivo e arquivos

- `numeric-core/contract_certificate.rs`: checker racional de certificado de
  sigmoid, enclosures composicionais e distância quadrática exata.
- `numeric-core/contract_adaptive.rs`: quatro políticas e trajetória completa
  de avaliações.
- `numeric-approximations/certified_sigmoid.rs`: geração de constantes e
  witnesses, separada do checker.
- `numeric-reference/transcendental.rs`: decisão dirigida/refinamento.
- `experiment-runner/bin/contract_audit.rs`: demonstração pequena exportável.
- `experiment-runner/bin/function_eval.rs`: resultados v2, proveniência
  conservadora GL, configuração efetiva, hashes de fontes e referência nullable.
- `experiments/cdad_contract_protocol_v1.json`, documentação do contrato e
  protocolo, README/reprodutibilidade e tratamento de flip inconclusivo no plot.

## Decisões e hipóteses

O novo contrato verifica uma relação racional exata com tolerâncias públicas.
Seu bound usa defeitos de momentos dos nós/pesos quantizados e uma cauda
geométrica. Assim, a correção não depende de confiar no gerador MPFR. É uma
extensão analítica nossa: não se atribui a Sridhar esse checker, adaptação ou
garantia de decisão. O domínio restrito y em [1/4,3/4] é obrigatório.

O estado validado não é desserializável. Certificados são desserializados e
reverificados. Isso não autentica a origem da entrada: compromisso vetorial
continua necessário para a garantia criptográfica pretendida.

## Execuções e evidência

```
cargo test --workspace --locked --quiet
cargo clippy --locked -p numeric-core -p numeric-reference -p numeric-approximations -p experiment-runner --lib --bins -- -D warnings
cargo run --locked -p experiment-runner --bin contract-audit -- --output-dir results/cdad_contract_audit_20260908_v1_verified
cargo run --locked -p experiment-runner --bin function-eval -- --output-dir results/functions_contract_audit_20260908_v2 --max-configurations 1 --grid-points 3 --random-samples 1 --timing-repetitions 0
python3 scripts/plot_non_polynomial.py --input-dir results/functions_contract_audit_20260908_v2 --output-dir results/functions_contract_audit_20260908_v2/figures
```

A suíte completa contém 168 testes passando. Cobertura nova: adulteração de
raio, saída, entrada, constantes, precisão e termos; round-trip dos certificados;
composição com slack; referência dirigida; contagem de todos os previews;
enumeração reduzida de witnesses de uma instância sigmoid com entrada zero.
Essa enumeração não cobre todos os inputs/domínios/perfis e não substitui o
argumento universal do contrato.

No controle de fronteira de distância quadrática, F16 fixo abstém; sequencial
decide em F48 após três avaliações; margem decide em F64 após duas; preview-all
seleciona F48 após avaliar quatro. Isso demonstra contabilização e uma previsão
que pode ultrapassar a menor precisão suficiente, não ganho de tempo.

A execução final `_verified` completou oito combinações workload/política.
Contratos e certificados coincidiram byte a byte com a execução preliminar;
todos os hashes de fontes do manifesto final foram reconferidos. A tentativa
de reutilizar um diretório existente foi rejeitada sem sobrescrita.

No controle de média sigmoid, todas as políticas selecionam F16 e decidem
False para tau=3/5; preview-all ainda executa quatro avaliações. Todos os
enclosures foram comparados à referência dirigida. Provas/constraints são null.

O smoke de funções teve 24 entradas e 72 decisões: 49 estáveis certificadas,
23 ambíguas, sete flips honestos (nenhum entre as decisões certificadas).
Somente exp/Taylor3/Q8.8/floor foi executado; não é uma comparação dos métodos.
Os sete gráficos são testes de exportação, não evidência de Pareto científico.

## Tentativas e correções durante o marco

- Um teste inicial escolheu como limiar o valor MPFR mais próximo, que podia
  coincidir com a borda inferior (decisão legitimamente conclusiva). Foi corrigido
  para usar o ponto médio estritamente interno do enclosure.
- A análise estática identificou construção ineficiente de hashes; corrigida
  sem alterar o conteúdo hexadecimal.
- A primeira execução de auditoria foi realizada durante ajustes do código.
  Seu diretório `cdad_contract_audit_20260908_v1` é preservado como preliminar;
  usar a execução `_verified`, iniciada com as fontes estabilizadas.

## Limitações e próximo passo

Nenhuma medição sustenta vantagem >=10%, nem os tempos do auditor são custos
completos de prova. R1CS/Nova das relações novas, compromisso vetorial, baseline
inteiro no backend, datasets finais e protocolo estatístico detalhado ainda
estão pendentes. A matriz congela requisitos e seeds, mas não substitui a
especificação ainda necessária de distribuição, unidades e split de dados.

Continuar pela revisão independente do argumento matemático e pela codificação
canônica vetorial ECEF. Somente após validar a correspondência circuito/checker
congelar a campanha final. Preservar resultados negativos e históricos. Outra
IA deve ler `docs/cdad-compositional-contract.md` e
`docs/cdad-competitive-protocol.md` antes de acrescentar qualquer alegação.
