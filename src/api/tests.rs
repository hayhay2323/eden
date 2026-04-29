use super::*;
use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceContract};
use crate::agent::llm::{AgentAnalysis, AgentNarration};
use crate::api::case_api::CaseTransitionAnalyticsQuery;
#[cfg(feature = "persistence")]
use crate::api::foundation::ApiState;
use crate::api::foundation::API_KEY_PREFIX;
use crate::cases::{
    CaseHumanReviewReasonStat, CaseMechanismDriftPoint, CaseMechanismStat,
    CaseMechanismTransitionDigest, CaseMechanismTransitionSliceStat, CaseMechanismTransitionStat,
    CaseReviewAnalytics, CaseReviewBuckets, CaseReviewMetrics, CaseReviewResponse,
};
use crate::live_snapshot::{LiveMarket, LiveMarketRegime, LiveScorecard, LiveStressSnapshot};
#[cfg(feature = "persistence")]
use crate::persistence::action_workflow::ActionWorkflowRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::tactical_setup::TacticalSetupRecord;
#[cfg(feature = "persistence")]
use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
#[cfg(feature = "persistence")]
use axum::Json;
use rust_decimal_macros::dec;
#[cfg(feature = "persistence")]
use std::net::{Ipv4Addr, SocketAddr};
#[cfg(feature = "persistence")]
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
#[cfg(feature = "persistence")]
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn encrypted_api_key_round_trip_works() {
    let cipher = ApiKeyCipher::from_secret("test-master-secret").expect("cipher");
    let minted = cipher
        .mint_key("frontend-app", 24, Some("frontend:readonly"))
        .expect("minted");
    assert!(minted.api_key.starts_with(API_KEY_PREFIX));

    let claims = cipher.validate(&minted.api_key).expect("claims");
    assert_eq!(claims.label, "frontend-app");
    assert_eq!(claims.scope, "frontend:readonly");
    assert!(claims.expires_at > claims.issued_at);
}

#[test]
fn invalid_prefix_is_rejected() {
    let cipher = ApiKeyCipher::from_secret("test-master-secret").expect("cipher");
    let error = cipher.validate("not-eden").expect_err("error");
    assert_eq!(error.status, StatusCode::UNAUTHORIZED);
}

#[test]
fn readonly_scope_blocks_mutations() {
    assert!(super::core::scope_allows_method(
        "frontend:readonly",
        &Method::GET
    ));
    assert!(!super::core::scope_allows_method(
        "frontend:readonly",
        &Method::POST
    ));
}

#[test]
fn write_scope_allows_mutations() {
    assert!(super::core::scope_allows_method(
        "frontend:write",
        &Method::POST
    ));
    assert!(super::core::scope_allows_method(
        "frontend:write",
        &Method::GET
    ));
}

#[test]
fn query_param_auth_is_rejected() {
    let headers = HeaderMap::new();
    assert!(super::core::extract_api_key(&headers).is_none());
}

#[test]
fn bearer_auth_is_accepted() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer eden_pk_test"),
    );
    assert_eq!(
        super::core::extract_api_key(&headers).as_deref(),
        Some("eden_pk_test")
    );
}

#[test]
fn x_api_key_header_is_accepted() {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", HeaderValue::from_static("eden_pk_header"));
    assert_eq!(
        super::core::extract_api_key(&headers).as_deref(),
        Some("eden_pk_header")
    );
}

#[test]
fn default_cors_uses_local_whitelist() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::remove_var("EDEN_API_ALLOWED_ORIGINS");
    let policy = super::core::resolve_cors_policy().expect("policy");
    assert_eq!(policy.mode, "default_local_whitelist");
    assert!(!policy.allow_any);
    assert!(policy
        .origins
        .iter()
        .any(|value| value == "http://127.0.0.1:5173"));
}

#[test]
fn explicit_cors_origins_override_default() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var(
        "EDEN_API_ALLOWED_ORIGINS",
        "http://localhost:9999,http://127.0.0.1:9998",
    );
    let policy = super::core::resolve_cors_policy().expect("policy");
    assert_eq!(policy.mode, "explicit_env");
    assert_eq!(
        policy.origins,
        vec![
            "http://localhost:9999".to_string(),
            "http://127.0.0.1:9998".to_string()
        ]
    );
    std::env::remove_var("EDEN_API_ALLOWED_ORIGINS");
}

#[test]
fn explicit_cors_star_keeps_any_mode() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("EDEN_API_ALLOWED_ORIGINS", "*");
    let policy = super::core::resolve_cors_policy().expect("policy");
    assert_eq!(policy.mode, "explicit_env");
    assert!(policy.allow_any);
    std::env::remove_var("EDEN_API_ALLOWED_ORIGINS");
}

#[cfg(feature = "persistence")]
fn temp_db_path(label: &str) -> PathBuf {
    let unique = format!(
        "eden-api-test-{}-{}-{}",
        label,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

#[cfg(feature = "persistence")]
fn sample_tactical_setup(setup_id: &str, workflow_id: &str) -> TacticalSetupRecord {
    TacticalSetupRecord {
        setup_id: setup_id.into(),
        hypothesis_id: "hyp:1".into(),
        runner_up_hypothesis_id: None,
        scope_key: "symbol:700.HK".into(),
        title: "Long 700".into(),
        action: "enter".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.70),
        confidence_gap: dec!(0.20),
        heuristic_edge: dec!(0.10),
        convergence_score: Some(dec!(0.55)),
        workflow_id: Some(workflow_id.into()),
        entry_rationale: "test".into(),
        risk_notes: vec![],
        case_signature: None,
        archetype_projections: vec![],
        expectation_bindings: vec![],
        expectation_violations: vec![],
        inferred_intent: None,
        primary_horizon: crate::ontology::horizon::HorizonBucket::Session,
        based_on: vec![],
        blocked_by: vec![],
        promoted_by: vec![],
        falsified_by: vec![],
        recorded_at: OffsetDateTime::UNIX_EPOCH,
    }
}

#[cfg(feature = "persistence")]
#[test]
fn workflow_must_start_from_suggest() {
    assert!(super::case_workflow_api::validate_transition(
        None,
        crate::action::workflow::ActionStage::Suggest,
    )
    .is_ok());
    assert!(super::case_workflow_api::validate_transition(
        None,
        crate::action::workflow::ActionStage::Confirm,
    )
    .is_err());
    assert!(super::case_workflow_api::validate_transition(
        None,
        crate::action::workflow::ActionStage::Review,
    )
    .is_err());
}

#[cfg(feature = "persistence")]
#[test]
fn wait_expectancy_sort_is_rejected() {
    let error =
        super::lineage_api::parse_sort_key(Some("wait")).expect_err("wait sort should be rejected");
    assert_eq!(error.status, StatusCode::BAD_REQUEST);

    let us_error = super::lineage_api::parse_us_lineage_sort_key(Some("wait"))
        .expect_err("US wait sort should be rejected");
    assert_eq!(us_error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn fresh_codex_prefers_loaded_final_narration() {
    let analysis = AgentAnalysis {
        tick: 100,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        status: "ok".into(),
        should_speak: true,
        provider: "codex-cloud".into(),
        model: "gpt-5.4".into(),
        message: Some("speak".into()),
        final_action: Some("speak".into()),
        steps: vec![],
        error: None,
    };
    let narration = AgentNarration {
        tick: 100,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        should_alert: true,
        alert_level: "high".into(),
        source: "codex-cloud".into(),
        headline: Some("headline".into()),
        message: Some("message".into()),
        bullets: vec![],
        focus_symbols: vec!["700.HK".into()],
        tags: vec![],
        primary_action: Some("buy".into()),
        confidence_band: Some("high".into()),
        what_changed: vec!["changed".into()],
        why_it_matters: Some("matters".into()),
        watch_next: vec![],
        what_not_to_do: vec![],
        fragility: vec![],
        recommendation_ids: vec![],
        market_summary_5m: Some("summary".into()),
        market_recommendation: Some(crate::agent::AgentMarketRecommendation {
            recommendation_id: "market:100:index".into(),
            tick: 100,
            market: LiveMarket::Hk,
            regime_bias: "neutral".into(),
            edge_layer: "market".into(),
            bias: "long".into(),
            best_action: "follow".into(),
            preferred_expression: "index".into(),
            market_impulse_score: dec!(0.82),
            macro_regime_discontinuity: dec!(0.74),
            expected_net_alpha: Some(dec!(0.01)),
            horizon_ticks: 20,
            alpha_horizon: "intraday:20t".into(),
            summary:
                "market-level long impulse detected; use index instead of forcing single names"
                    .into(),
            why_not_single_name: Some(
                "index lift dominates idiosyncratic edge; keep single names selective".into(),
            ),
            focus_sectors: vec!["Finance".into(), "Property".into()],
            decisive_factors: vec!["breadth up=84% down=8% avg_return=+2.20%".into()],
            reference_symbols: vec!["700.HK".into(), "5.HK".into()],
            average_return_at_decision: dec!(0.02),
            resolution: None,
            execution_policy: ActionExecutionPolicy::ManualOnly,
            governance: ActionGovernanceContract::for_recommendation(
                ActionExecutionPolicy::ManualOnly,
            ),
            governance_reason_code:
                crate::action::workflow::ActionGovernanceReasonCode::AdvisoryAction,
            governance_reason:
                "market-level recommendations stay advisory until an explicit workflow exists"
                    .into(),
        }),
        dominant_lenses: vec![crate::agent::llm::AgentDominantLens {
            lens_name: "iceberg".into(),
            card_count: 1,
            max_confidence: dec!(0.72),
            mean_confidence: dec!(0.72),
        }],
        action_cards: vec![crate::agent::llm::AgentNarrationActionCard {
            card_id: "card:100:700.HK".into(),
            scope_kind: "symbol".into(),
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            edge_layer: None,
            setup_id: Some("setup-1".into()),
            action: "buy".into(),
            action_label: Some("買入".into()),
            severity: "high".into(),
            title: None,
            summary: "summary".into(),
            why_now: "why".into(),
            why_components: vec![crate::agent::AgentLensComponent {
                lens_name: "iceberg".into(),
                confidence: dec!(0.72),
                content: "偵測到3次冰山回補".into(),
                tags: vec!["iceberg".into(), "broker".into()],
            }],
            primary_lens: Some("iceberg".into()),
            supporting_lenses: vec!["causal".into()],
            review_lens: Some("iceberg".into()),
            confidence_band: Some("high".into()),
            watch_next: vec![],
            do_not: vec![],
            thesis_family: Some("Directed Flow".into()),
            state_transition: Some("review -> enter".into()),
            best_action: "follow".into(),
            action_expectancies: crate::agent::AgentActionExpectancies {
                follow_expectancy: Some(dec!(0.03)),
                fade_expectancy: Some(dec!(-0.02)),
                wait_expectancy: Some(dec!(0)),
            },
            decision_attribution: crate::agent::AgentDecisionAttribution {
                historical_expectancies: crate::agent::AgentActionExpectancies {
                    follow_expectancy: Some(dec!(0.025)),
                    fade_expectancy: Some(dec!(-0.015)),
                    wait_expectancy: Some(dec!(0)),
                },
                live_expectancy_shift: dec!(0.005),
                decisive_factors: vec![
                    "historical prior follow=+2.50% fade=-1.50% wait=+0.00%".into(),
                    "live shift +0.50% on follow, -0.50% on fade".into(),
                ],
            },
            expected_net_alpha: Some(dec!(0.03)),
            alpha_horizon: "intraday:10t".into(),
            preferred_expression: None,
            reference_symbols: vec!["700.HK".into()],
            invalidation_rule: Some("institutional alignment flips negative".into()),
            invalidation_components: vec![crate::agent::AgentLensComponent {
                lens_name: "causal".into(),
                confidence: dec!(0.68),
                content: "institutional alignment flips negative".into(),
                tags: vec!["causal".into()],
            }],
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code:
                crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview,
            governance_reason: "severity=`high` forces human review before `buy` can execute"
                .into(),
        }],
    };

    assert!(super::agent_surface::should_return_loaded_final_narration(
        104,
        Some(&analysis),
        Some(&narration)
    ));
}

#[test]
fn transition_analytics_response_filters_and_limits() {
    let review = CaseReviewResponse {
        context: crate::cases::CaseMarketContext {
            market: LiveMarket::Us,
            tick: 42,
            timestamp: "2026-03-22T00:00:00Z".into(),
            stock_count: 0,
            edge_count: 0,
            hypothesis_count: 0,
            observation_count: 0,
            active_positions: 0,
            market_regime: LiveMarketRegime {
                bias: "risk_off".into(),
                confidence: dec!(0.7),
                breadth_up: dec!(0.2),
                breadth_down: dec!(0.6),
                average_return: dec!(-0.03),
                directional_consensus: Some(dec!(-0.1)),
                pre_market_sentiment: None,
            },
            stress: LiveStressSnapshot {
                composite_stress: dec!(0.5),
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            },
            scorecard: LiveScorecard::default(),
            events: vec![],
            cross_market_signals: vec![],
            cross_market_anomalies: vec![],
            lineage: vec![],
        },
        metrics: CaseReviewMetrics {
            in_flight: 0,
            under_review: 0,
            at_risk: 0,
            high_conviction: 0,
            manual_only: 0,
            review_required: 0,
            auto_eligible: 0,
            queue_pinned: 0,
        },
        buckets: CaseReviewBuckets {
            in_flight: vec![],
            under_review: vec![],
            at_risk: vec![],
            high_conviction: vec![],
        },
        governance_buckets: crate::cases::CaseGovernanceBuckets::default(),
        governance_reason_buckets: crate::cases::CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: crate::cases::CasePrimaryLensBuckets::default(),
        queue_pin_buckets: crate::cases::CaseQueuePinBuckets::default(),
        analytics: CaseReviewAnalytics {
            mechanism_stats: vec![CaseMechanismStat {
                mechanism: "Capital Rotation".into(),
                cases: 1,
                under_review: 0,
                at_risk: 0,
                high_conviction: 1,
                avg_score: dec!(0.6),
            }],
            intent_stats: vec![],
            intent_state_stats: vec![],
            intent_exit_signal_stats: vec![],
            intent_opportunity_stats: vec![],
            intent_adjustments: vec![],
            review_required_by_lens: vec![],
            human_override_by_lens: vec![],
            lens_regime_hit_rates: vec![],
            archetype_stats: vec![],
            discovered_archetype_catalog: vec![],
            signature_stats: vec![],
            expectation_violation_stats: vec![],
            intelligence_signals: crate::cases::CaseIntelligenceSignals::default(),
            memory_impact: vec![],
            violation_predictiveness: vec![],
            reviewer_corrections: vec![],
            mechanism_drift: vec![CaseMechanismDriftPoint {
                window_label: "03-22 10:00".into(),
                top_mechanism: Some("Capital Rotation".into()),
                top_cases: 1,
                avg_score: dec!(0.6),
                dominant_factor: Some("Substitution Flow".into()),
            }],
            mechanism_transition_breakdown: vec![
                CaseMechanismTransitionStat {
                    classification: "regime_shift".into(),
                    count: 2,
                },
                CaseMechanismTransitionStat {
                    classification: "mechanism_decay".into(),
                    count: 1,
                },
            ],
            transition_by_sector: vec![
                CaseMechanismTransitionSliceStat {
                    key: "Technology".into(),
                    classification: "regime_shift".into(),
                    count: 2,
                },
                CaseMechanismTransitionSliceStat {
                    key: "Financials".into(),
                    classification: "mechanism_decay".into(),
                    count: 1,
                },
            ],
            transition_by_regime: vec![
                CaseMechanismTransitionSliceStat {
                    key: "risk_off:high".into(),
                    classification: "regime_shift".into(),
                    count: 2,
                },
                CaseMechanismTransitionSliceStat {
                    key: "neutral:low".into(),
                    classification: "mechanism_decay".into(),
                    count: 1,
                },
            ],
            transition_by_reviewer: vec![CaseMechanismTransitionSliceStat {
                key: "reviewer-a".into(),
                classification: "regime_shift".into(),
                count: 1,
            }],
            recent_mechanism_transitions: vec![
                CaseMechanismTransitionDigest {
                    setup_id: "setup:1".into(),
                    symbol: "A.US".into(),
                    title: "A".into(),
                    sector: Some("Technology".into()),
                    regime: Some("risk_off:high".into()),
                    reviewer: Some("reviewer-a".into()),
                    from_mechanism: Some("Mechanical Execution Signature".into()),
                    to_mechanism: Some("Capital Rotation".into()),
                    classification: "regime_shift".into(),
                    confidence: dec!(0.82),
                    summary: "shift".into(),
                    recorded_at: OffsetDateTime::UNIX_EPOCH,
                },
                CaseMechanismTransitionDigest {
                    setup_id: "setup:2".into(),
                    symbol: "B.US".into(),
                    title: "B".into(),
                    sector: Some("Financials".into()),
                    regime: Some("neutral:low".into()),
                    reviewer: Some("reviewer-b".into()),
                    from_mechanism: Some("Narrative Failure".into()),
                    to_mechanism: Some("Fragility Build-up".into()),
                    classification: "mechanism_decay".into(),
                    confidence: dec!(0.61),
                    summary: "decay".into(),
                    recorded_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
                },
            ],
            reviewer_doctrine: vec![],
            human_review_reasons: vec![CaseHumanReviewReasonStat {
                reason: "Mechanism Mismatch".into(),
                count: 1,
            }],
            invalidation_patterns: vec![],
            review_reason_feedback: vec![],
            review_reason_family_feedback: vec![],
            learning_feedback: crate::pipeline::learning_loop::ReasoningLearningFeedback::default(),
        },
    };

    let response = super::case_api::build_case_transition_analytics_response(
        &review,
        &CaseTransitionAnalyticsQuery {
            classification: Some("regime_shift".into()),
            queue_pin: Some("frontend-review-list".into()),
            primary_lens: Some("iceberg".into()),
            governance_reason_code: Some(
                crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview,
            ),
            limit: Some(1),
            ..CaseTransitionAnalyticsQuery::default()
        },
    );

    assert_eq!(response.market, "us");
    assert_eq!(
        response.filters.queue_pin.as_deref(),
        Some("frontend-review-list")
    );
    assert_eq!(response.filters.primary_lens.as_deref(), Some("iceberg"));
    assert_eq!(
        response.filters.governance_reason_code,
        Some(crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview)
    );
    assert_eq!(response.mechanism_transition_breakdown.len(), 1);
    assert_eq!(response.transition_by_sector.len(), 1);
    assert_eq!(response.transition_by_regime.len(), 1);
    assert_eq!(response.transition_by_reviewer.len(), 1);
    assert_eq!(response.recent_mechanism_transitions.len(), 1);
    assert_eq!(
        response.recent_mechanism_transitions[0].classification,
        "regime_shift"
    );
}

#[cfg(feature = "persistence")]
#[tokio::test]
async fn post_case_assign_rejects_queue_pin_owned_by_another_actor() {
    let path = temp_db_path("queue-pin-conflict");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
    let setup = sample_tactical_setup("setup:queue-pin", "wf:queue-pin");
    store.write_tactical_setup(&setup).await.unwrap();
    store
        .write_action_workflow(&ActionWorkflowRecord {
            workflow_id: "wf:queue-pin".into(),
            title: "Queue Pin".into(),
            payload: serde_json::json!({ "setup_id": "setup:queue-pin" }),
            current_stage: crate::action::workflow::ActionStage::Suggest,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code:
                crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            actor: Some("frontend-review-list".into()),
            owner: None,
            reviewer: None,
            queue_pin: Some("frontend-review-list".into()),
            note: Some("pinned".into()),
        })
        .await
        .unwrap();

    let auth = ApiKeyCipher::from_secret("test-master-secret").expect("cipher");
    let revocations = ApiKeyRevocationStore::load(path.join("revocations.json").to_str().unwrap())
        .expect("revocations");
    let state = ApiState {
        bind_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 8787)),
        auth,
        revocations,
        runtime_tasks: crate::core::runtime_tasks::RuntimeTaskStore::load(
            path.join("runtime_tasks.json"),
        )
        .expect("runtime task store"),
        store: store.clone(),
    };

    let result = super::case_workflow_api::post_case_assign(
        State(state),
        Path(("hk".to_string(), "setup:queue-pin".to_string())),
        Json(super::case_workflow_api::CaseAssignBody {
            owner: None,
            reviewer: None,
            queue_pin: Some(Some("ops-desk".into())),
            actor: Some("frontend".into()),
            note: None,
        }),
    )
    .await;

    let error = result.expect_err("queue pin reassignment should be rejected");
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error
        .message
        .contains("queue pin is owned by `frontend-review-list`"));

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[cfg(feature = "persistence")]
#[tokio::test]
async fn post_case_queue_pin_sets_and_clears_marker() {
    let path = temp_db_path("queue-pin-endpoint");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
    let setup = sample_tactical_setup("setup:queue-pin-endpoint", "wf:queue-pin-endpoint");
    store.write_tactical_setup(&setup).await.unwrap();

    let auth = ApiKeyCipher::from_secret("test-master-secret").expect("cipher");
    let revocations = ApiKeyRevocationStore::load(path.join("revocations.json").to_str().unwrap())
        .expect("revocations");
    let state = ApiState {
        bind_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 8787)),
        auth,
        revocations,
        runtime_tasks: crate::core::runtime_tasks::RuntimeTaskStore::load(
            path.join("runtime_tasks.json"),
        )
        .expect("runtime task store"),
        store: store.clone(),
    };

    let pinned = super::case_workflow_api::post_case_queue_pin(
        State(state.clone()),
        Path(("hk".to_string(), "setup:queue-pin-endpoint".to_string())),
        Json(super::case_workflow_api::CaseQueuePinBody {
            pinned: true,
            label: None,
            actor: Some("frontend-review-list".into()),
            note: None,
        }),
    )
    .await
    .expect("queue pin set");

    assert_eq!(pinned.0.queue_pin.as_deref(), Some("frontend-review-list"));
    assert_eq!(pinned.0.stage, "suggest");
    assert!(pinned.0.note.as_deref().unwrap_or("").contains("queue pin"));

    let stored = store
        .action_workflow_by_id("wf:queue-pin-endpoint")
        .await
        .expect("workflow lookup")
        .expect("workflow record");
    assert_eq!(stored.queue_pin.as_deref(), Some("frontend-review-list"));

    let cleared = super::case_workflow_api::post_case_queue_pin(
        State(state),
        Path(("hk".to_string(), "setup:queue-pin-endpoint".to_string())),
        Json(super::case_workflow_api::CaseQueuePinBody {
            pinned: false,
            label: None,
            actor: Some("frontend-review-list".into()),
            note: None,
        }),
    )
    .await
    .expect("queue pin clear");

    assert_eq!(cleared.0.queue_pin, None);
    assert!(cleared
        .0
        .note
        .as_deref()
        .unwrap_or("")
        .contains("queue pin cleared"));

    let stored = store
        .action_workflow_by_id("wf:queue-pin-endpoint")
        .await
        .expect("workflow lookup after clear")
        .expect("workflow record after clear");
    assert_eq!(stored.queue_pin, None);

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[cfg(feature = "persistence")]
#[test]
fn queue_pin_conflict_rule_matches_api_expectation() {
    let error = crate::action::workflow::validate_queue_pin_update(
        Some("frontend-review-list"),
        Some(&Some("ops-desk".into())),
        Some("frontend"),
    )
    .expect_err("queue pin reassignment should be rejected");
    let api_error = ApiError::bad_request(error.to_string());
    assert_eq!(api_error.status, StatusCode::BAD_REQUEST);
    assert!(api_error
        .message
        .contains("queue pin is owned by `frontend-review-list`"));
}
