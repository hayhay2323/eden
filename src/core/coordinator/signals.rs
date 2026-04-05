use super::types::{CrossMarketDivergence, CrossMarketHypothesis, DivergenceSeverity};

/// Analyzes divergences between HK and US market states.
pub struct CrossMarketAnalyzer;

impl CrossMarketAnalyzer {
    /// Detect regime divergences between markets.
    pub fn detect_divergences(
        hk_regime: Option<&str>,
        us_regime: Option<&str>,
        hk_stress: Option<f64>,
        us_stress: Option<f64>,
    ) -> Vec<CrossMarketDivergence> {
        let mut divergences = Vec::new();

        // Regime divergence: one market in a different regime than the other.
        if let (Some(hk), Some(us)) = (hk_regime, us_regime) {
            if hk != us {
                divergences.push(CrossMarketDivergence {
                    kind: "regime_divergence".into(),
                    description: format!("HK regime '{}' differs from US regime '{}'", hk, us),
                    hk_value: None,
                    us_value: None,
                    severity: DivergenceSeverity::Medium,
                    detected_at: String::new(),
                });
            }
        }

        // Stress level divergence — flag when the relative difference between
        // the two stress readings is large compared to their combined magnitude.
        if let (Some(hk_s), Some(us_s)) = (hk_stress, us_stress) {
            let diff = (hk_s - us_s).abs();
            let magnitude = (hk_s.abs() + us_s.abs()) / 2.0;
            if magnitude > 0.0 && diff / magnitude > 0.5 {
                divergences.push(CrossMarketDivergence {
                    kind: "stress_divergence".into(),
                    description: format!(
                        "Stress divergence: HK={:.2}, US={:.2} (relative diff {:.0}%)",
                        hk_s,
                        us_s,
                        (diff / magnitude) * 100.0
                    ),
                    hk_value: Some(hk_s),
                    us_value: Some(us_s),
                    severity: if diff / magnitude > 0.8 {
                        DivergenceSeverity::High
                    } else {
                        DivergenceSeverity::Medium
                    },
                    detected_at: String::new(),
                });
            }
        }

        divergences
    }

    /// Generate cross-market hypotheses from detected divergences.
    pub fn generate_hypotheses(
        divergences: &[CrossMarketDivergence],
    ) -> Vec<CrossMarketHypothesis> {
        let mut hypotheses = Vec::new();

        for (i, div) in divergences.iter().enumerate() {
            if div.severity == DivergenceSeverity::High {
                hypotheses.push(CrossMarketHypothesis {
                    id: format!("xmkt-{}", i),
                    label: format!("Cross-market {} signal", div.kind),
                    description: div.description.clone(),
                    confidence: match div.severity {
                        DivergenceSeverity::High => 0.7,
                        DivergenceSeverity::Medium => 0.5,
                        DivergenceSeverity::Low => 0.3,
                    },
                    supporting_markets: vec!["HK".into(), "US".into()],
                });
            }
        }

        hypotheses
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_regime_divergence() {
        let divs =
            CrossMarketAnalyzer::detect_divergences(Some("bullish"), Some("bearish"), None, None);
        assert_eq!(divs.len(), 1);
        assert_eq!(divs[0].kind, "regime_divergence");
    }

    #[test]
    fn no_divergence_same_regime() {
        let divs =
            CrossMarketAnalyzer::detect_divergences(Some("bullish"), Some("bullish"), None, None);
        assert!(divs.is_empty());
    }

    #[test]
    fn detects_stress_divergence() {
        let divs = CrossMarketAnalyzer::detect_divergences(None, None, Some(0.8), Some(0.2));
        assert!(
            !divs.is_empty(),
            "Should detect stress divergence between 0.8 and 0.2"
        );
        assert_eq!(divs[0].kind, "stress_divergence");
    }
}
