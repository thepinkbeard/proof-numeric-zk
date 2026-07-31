//! Soundness por enumeração exaustiva em formato reduzido.
//!
//! Todo o resto do projeto testa as representações certificadas por amostragem:
//! 10.000 entradas aqui, 200.000 ali. Amostragem não demonstra soundness, mostra
//! ausência de contraexemplo na amostra. A propriedade que o capítulo afirma é
//! universal — *nenhuma* decisão definitiva é incorreta — e uma afirmação
//! universal precisa de um argumento universal.
//!
//! A saída, mais barata e mais forte que fuzzing, é encolher o formato até que a
//! enumeração completa caiba. Com poucos bits fracionários o conjunto de
//! enclausuramentos representáveis é finito e pequeno, e dá para percorrer todos.
//!
//! ## Por que isto é exaustivo de verdade, e não amostragem fina
//!
//! Há dois quantificadores no enunciado: "para todo enclausuramento
//! representável" e "para todo real dentro dele". O primeiro se resolve por
//! enumeração. O segundo não — o intervalo é contínuo.
//!
//! O que fecha o segundo é o mapa logístico ser contínuo e unimodal em `[0,1]`,
//! com máximo em `1/2`. A imagem de `[a,b]` é então exatamente
//! `[min(f(a),f(b)), max(f(a), f(b), f(1/2) se 1/2 ∈ [a,b])]`. Basta conferir
//! que esses extremos exatos caem dentro do enclausuramento de saída, e a
//! contenção vale para o contínuo inteiro. É verificação completa, não
//! discretização fina.

use num_bigint::{BigInt, BigUint};
use num_rational::BigRational;
use num_traits::One;
use numeric_core::{
    CbdError, CertifiedDyadic, CertifiedFloat, CfdOperation, CfdProfile, Decision3, NumericProfile,
};

/// Bits fracionários do formato reduzido. Com 6 bits são 65 mantissas em `[0,1]`
/// e raios até 8, o que dá algumas centenas de enclausuramentos por variante —
/// exaustivo e rápido o bastante para rodar em cada commit.
const REDUCED_BITS: usize = 6;
const MAX_RADIUS: u64 = 8;

/// Assinatura comum às cinco variantes de passo logístico do CBD.
type LogisticStep = fn(&CertifiedDyadic, &BigRational) -> Result<CertifiedDyadic, CbdError>;

fn reduced_profile() -> NumericProfile {
    NumericProfile::new(
        format!("cbd-reduced-{REDUCED_BITS}"),
        1,
        REDUCED_BITS,
        REDUCED_BITS + 4,
        BigRational::from_integer(0.into()),
        BigRational::from_integer(1.into()),
        vec![BigRational::new(1.into(), 2.into())],
        Some(REDUCED_BITS + 4),
    )
    .expect("perfil reduzido válido")
}

fn ratio(numerator: i64, denominator: i64) -> BigRational {
    BigRational::new(numerator.into(), denominator.into())
}

/// Imagem exata de `[lower, upper]` sob `x -> r*x*(1-x)`.
///
/// Completa por unimodalidade: o único ponto crítico em `[0,1]` é `1/2`.
fn exact_logistic_image(
    lower: &BigRational,
    upper: &BigRational,
    r: &BigRational,
) -> (BigRational, BigRational) {
    let f = |x: &BigRational| r * x * (BigRational::one() - x);
    let at_lower = f(lower);
    let at_upper = f(upper);
    let half = ratio(1, 2);
    let mut minimum = at_lower.clone().min(at_upper.clone());
    let mut maximum = at_lower.max(at_upper);
    if *lower <= half && half <= *upper {
        let at_half = f(&half);
        minimum = minimum.min(at_half.clone());
        maximum = maximum.max(at_half);
    }
    (minimum, maximum)
}

/// Todos os enclausuramentos CBD representáveis no formato reduzido.
fn all_cbd_enclosures(profile: &NumericProfile) -> Vec<CertifiedDyadic> {
    let scale = 1_u64 << REDUCED_BITS;
    let mut values = Vec::new();
    for mantissa in 0..=scale {
        for radius in 0..=MAX_RADIUS {
            if let Ok(value) = CertifiedDyadic::from_parts(
                profile.clone(),
                BigInt::from(mantissa),
                BigUint::from(radius),
            ) {
                values.push(value);
            }
        }
    }
    values
}

#[test]
fn cbd_enclosures_are_exhaustively_enumerable() {
    let profile = reduced_profile();
    let values = all_cbd_enclosures(&profile);
    // 65 mantissas x 9 raios, menos os que saem do domínio público.
    assert!(
        values.len() > 100,
        "enumeração pequena demais para ser significativa: {}",
        values.len()
    );
}

/// A propriedade central: a saída certificada contém a imagem exata da entrada.
///
/// Se isto falhar para algum enclausuramento, a representação pode produzir uma
/// decisão definitiva incorreta, e o argumento inteiro do capítulo cai.
#[test]
fn every_cbd_logistic_step_encloses_the_exact_image() {
    let profile = reduced_profile();
    let r = ratio(37, 10);
    let variants: [(&str, LogisticStep); 5] = [
        ("fused", CertifiedDyadic::logistic_step_fused),
        ("sequential", CertifiedDyadic::logistic_step_sequential),
        ("lipschitz", CertifiedDyadic::logistic_step_lipschitz),
        ("local", CertifiedDyadic::logistic_step_local_bound),
        (
            "sequential-local",
            CertifiedDyadic::logistic_step_sequential_local_bound,
        ),
    ];

    let mut checked = 0_u32;
    for value in all_cbd_enclosures(&profile) {
        let (exact_low, exact_high) = exact_logistic_image(&value.lower(), &value.upper(), &r);
        for (name, step) in variants {
            let Ok(output) = step(&value, &r) else {
                // Estouro do domínio público é recusa explícita, não resultado
                // errado: o passo não produziu enclausuramento nenhum.
                continue;
            };
            assert!(
                output.lower() <= exact_low && exact_high <= output.upper(),
                "{name}: saída [{}, {}] não contém a imagem exata [{}, {}] da entrada [{}, {}]",
                output.lower(),
                output.upper(),
                exact_low,
                exact_high,
                value.lower(),
                value.upper()
            );
            checked += 1;
        }
    }
    assert!(checked > 500, "cobertura insuficiente: {checked}");
}

/// Nenhuma decisão definitiva contradiz a imagem exata.
///
/// Esta é a forma que o resultado ECEF assume como teorema: quando o
/// enclausuramento não separa o limiar, a resposta é `Indeterminate`; quando é
/// `True` ou `False`, concorda com todo real do intervalo.
#[test]
fn no_cbd_definite_decision_contradicts_the_exact_image() {
    let profile = reduced_profile();
    let r = ratio(37, 10);
    let threshold = ratio(1, 2);
    let mut definite = 0_u32;

    for value in all_cbd_enclosures(&profile) {
        let Ok(output) = value.logistic_step_local_bound(&r) else {
            continue;
        };
        let (exact_low, exact_high) = exact_logistic_image(&value.lower(), &value.upper(), &r);
        match output.ge_threshold(&threshold) {
            Decision3::True => {
                // Definitiva verdadeira exige que *todo* real da imagem esteja
                // acima do limiar, e o mínimo é quem testemunha isso.
                assert!(
                    exact_low >= threshold,
                    "decidiu True mas a imagem exata desce a {exact_low}"
                );
                definite += 1;
            }
            Decision3::False => {
                assert!(
                    exact_high < threshold,
                    "decidiu False mas a imagem exata sobe a {exact_high}"
                );
                definite += 1;
            }
            Decision3::Indeterminate => {}
        }
    }
    assert!(definite > 0, "nenhuma decisão definitiva para verificar");
}

/// Iterar não escapa: a soundness precisa valer na composição, não só num passo.
#[test]
fn cbd_soundness_survives_iteration() {
    let profile = reduced_profile();
    let r = ratio(37, 10);
    let scale = 1_u64 << REDUCED_BITS;

    for mantissa in 0..=scale {
        let Ok(mut value) =
            CertifiedDyadic::from_parts(profile.clone(), BigInt::from(mantissa), BigUint::from(0_u64))
        else {
            continue;
        };
        // O exato acompanha o centro inicial, que é exatamente representável.
        let mut exact_low = value.lower();
        let mut exact_high = value.upper();

        for step in 0..8 {
            let Ok(next) = value.logistic_step_local_bound(&r) else {
                break;
            };
            let (image_low, image_high) = exact_logistic_image(&exact_low, &exact_high, &r);
            assert!(
                next.lower() <= image_low && image_high <= next.upper(),
                "passo {step} perdeu a contenção para mantissa {mantissa}"
            );
            exact_low = image_low;
            exact_high = image_high;
            value = next;
        }
    }
}

fn reduced_cfd_profile() -> CfdProfile {
    CfdProfile::new(
        "cfd-reduced",
        1,
        8,
        (-6..=0).collect(),
        8,
        BigRational::from_integer(0.into()),
        BigRational::from_integer(1.into()),
        vec![CfdOperation::LogisticSequential, CfdOperation::LogisticFused],
        vec![ratio(1, 2)],
        None,
    )
    .expect("perfil CFD reduzido válido")
}

/// A mesma propriedade para CFD, enumerando a grade de valores exatos que o
/// formato representa.
///
/// A enumeração aqui é sobre pontos e não sobre intervalos: o enclausuramento
/// CFD é construído a partir do exato, e é a partir dele que a iteração alarga.
/// A iteração cobre o caso intervalar.
#[test]
fn every_cfd_logistic_step_encloses_the_exact_image() {
    let profile = reduced_cfd_profile();
    let r = ratio(37, 10);
    let grid = 1_i64 << REDUCED_BITS;
    let mut checked = 0_u32;

    for numerator in 0..=grid {
        let exact = BigRational::new(numerator.into(), grid.into());
        let Ok(value) = CertifiedFloat::from_exact(profile.clone(), &exact) else {
            continue;
        };
        assert!(
            value.contains(&exact),
            "o enclausuramento inicial já não contém {exact}"
        );

        for (name, output) in [
            ("sequential", value.logistic_step_sequential(&r)),
            ("fused", value.logistic_step_fused(&r)),
        ] {
            let Ok(output) = output else { continue };
            if output.is_wide() {
                // `wide` é abstenção declarada, e abstenção nunca é unsound.
                continue;
            }
            let (image_low, image_high) =
                exact_logistic_image(&value.lower(), &value.upper(), &r);
            assert!(
                output.lower() <= image_low && image_high <= output.upper(),
                "{name}: saída [{}, {}] não contém [{}, {}]",
                output.lower(),
                output.upper(),
                image_low,
                image_high
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "cobertura CFD insuficiente: {checked}");
}

#[test]
fn cfd_soundness_survives_iteration() {
    let profile = reduced_cfd_profile();
    let r = ratio(37, 10);
    let grid = 1_i64 << REDUCED_BITS;

    for numerator in 0..=grid {
        let exact = BigRational::new(numerator.into(), grid.into());
        let Ok(mut value) = CertifiedFloat::from_exact(profile.clone(), &exact) else {
            continue;
        };
        let mut exact_low = value.lower();
        let mut exact_high = value.upper();

        for step in 0..8 {
            let Ok(next) = value.logistic_step_sequential(&r) else {
                break;
            };
            if next.is_wide() {
                break;
            }
            let (image_low, image_high) = exact_logistic_image(&exact_low, &exact_high, &r);
            assert!(
                next.lower() <= image_low && image_high <= next.upper(),
                "passo {step} perdeu a contenção para {numerator}/{grid}"
            );
            exact_low = image_low;
            exact_high = image_high;
            value = next;
        }
    }
}

/// O contraste com o formato não certificado, no mesmo formato reduzido.
///
/// Ponto fixo com a mesma grade responde sempre, e este teste conta quantas
/// vezes responde errado. É o argumento do capítulo reduzido a um caso que cabe
/// inteiro na cabeça: não é que o certificado seja mais preciso, é que o outro
/// não sabe quando está errado.
#[test]
fn fixed_point_in_the_same_grid_produces_wrong_definite_decisions() {
    let r = ratio(37, 10);
    let threshold = ratio(1, 2);
    let grid = 1_i64 << REDUCED_BITS;
    let scale = BigRational::from_integer(grid.into());
    let mut wrong = 0_u32;

    for numerator in 0..=grid {
        let exact = BigRational::new(numerator.into(), grid.into());

        // Ponto fixo: arredonda para a grade a cada passo, e decide sempre.
        let mut approximate = exact.clone();
        let mut truth = exact.clone();
        for _ in 0..8 {
            let stepped = &r * &approximate * (BigRational::one() - &approximate);
            let scaled = &stepped * &scale;
            let rounded = scaled.round();
            approximate = rounded / &scale;
            truth = &r * &truth * (BigRational::one() - &truth);
            if truth < BigRational::from_integer(0.into())
                || truth > BigRational::from_integer(1.into())
            {
                break;
            }
        }
        if (approximate >= threshold) != (truth >= threshold) {
            wrong += 1;
        }
    }

    assert!(
        wrong > 0,
        "o ponto fixo acertou tudo na grade reduzida; o contraste do capítulo \
         precisa de outro parâmetro para ser demonstrável"
    );
    println!("ponto fixo reduzido: {wrong} de {} decisões incorretas", grid + 1);
}
