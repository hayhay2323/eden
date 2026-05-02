//! Architecture invariants for the V2 sub-KG-driven BP fusion.
//!
//! These tests don't exercise behaviour — they enforce structural
//! properties of the pipeline so future refactors can't quietly
//! reintroduce the patterns the V2 plan deleted.
//!
//! If a test here starts failing, the invariant is broken and we need
//! to either delete the offending code or update this test deliberately.

#![cfg(test)]

use std::fs;
use std::path::Path;

fn pipeline_files() -> Vec<String> {
    walk_files("src/pipeline")
}

fn runtime_files() -> Vec<String> {
    let mut out = Vec::new();
    out.extend(walk_files("src/us/runtime"));
    out.extend(walk_files("src/hk/runtime"));
    out.push("src/us/runtime.rs".to_string());
    out.push("src/hk/runtime.rs".to_string());
    out
}

/// Files exempt from invariant pattern scans. The invariants test
/// itself necessarily references the banned strings as needles.
const SELF_PATH: &str = "architecture_invariants_tests.rs";

fn walk_files(dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let path = Path::new(dir);
    if !path.exists() {
        return out;
    }
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(entries) = fs::read_dir(&p) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Some(s) = path.to_str() {
                    if !s.ends_with(SELF_PATH) {
                        out.push(s.to_string());
                    }
                }
            }
        }
    }
    out
}

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// Strip line / block comments before pattern-matching so commit-message
/// references in doc comments don't trip invariants.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_block = false;
    for line in src.lines() {
        let mut buf = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if in_block {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    in_block = false;
                }
                continue;
            }
            if c == '/' {
                if chars.peek() == Some(&'/') {
                    break; // rest of line is comment
                }
                if chars.peek() == Some(&'*') {
                    chars.next();
                    in_block = true;
                    continue;
                }
            }
            buf.push(c);
        }
        out.push_str(&buf);
        out.push('\n');
    }
    out
}

fn strip_cfg_test_modules(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut pending_cfg_test = false;
    let mut skipping_test_item = false;
    let mut brace_depth = 0i32;

    for line in src.lines() {
        let trimmed = line.trim_start();

        if skipping_test_item {
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;
            if brace_depth <= 0 {
                skipping_test_item = false;
                brace_depth = 0;
            }
            continue;
        }

        if pending_cfg_test {
            if trimmed.starts_with("mod ") && line.contains('{') {
                brace_depth = line.matches('{').count() as i32 - line.matches('}').count() as i32;
                skipping_test_item = brace_depth > 0;
                pending_cfg_test = false;
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with("#[") {
                continue;
            }
            pending_cfg_test = false;
        }

        if trimmed.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

fn contains_outside_comments(path: &str, needle: &str) -> bool {
    strip_comments(&read(path)).contains(needle)
}

fn assert_marker_order(path: &str, markers: &[(&str, &str)]) {
    let stripped = strip_comments(&read(path));
    let mut last_idx = None;
    let mut last_label = "";
    for (label, needle) in markers {
        let Some(idx) = stripped.find(needle) else {
            panic!("{path} must contain runtime marker `{label}` via `{needle}`");
        };
        if let Some(prev) = last_idx {
            assert!(
                idx > prev,
                "{path} runtime order violation: `{label}` must appear after `{last_label}`",
            );
        }
        last_idx = Some(idx);
        last_label = label;
    }
}

#[test]
fn modulation_chain_modules_are_deleted() {
    for stale in [
        "src/pipeline/belief_modulation.rs",
        "src/pipeline/outcome_history.rs",
        "src/pipeline/modulation_report.rs",
        "src/pipeline/closed_loop_tests.rs",
        "src/pipeline/engram_modulation.rs",
        "src/pipeline/wl_signature_modulation.rs",
        "src/pipeline/bp_modulation.rs",
        "src/pipeline/lead_lag_modulation.rs",
    ] {
        assert!(
            !Path::new(stale).exists(),
            "V2 invariant: {} must not exist (modulation chain replaced by sub-KG fusion)",
            stale,
        );
    }
}

#[test]
fn no_modulation_chain_function_calls_in_runtime() {
    // No file outside of comments may call apply_*_modulation functions —
    // they were the modulation chain entries deleted in Phase 2.
    let stale_calls = [
        "apply_belief_modulation(",
        "apply_outcome_history_modulation(",
        "apply_outcome_history_modulation_with_synthetic(",
        "apply_engram_modulation(",
        "apply_wl_signature_modulation(",
        "apply_bp_modulation(",
        "apply_lead_lag_modulation(",
    ];
    for path in pipeline_files().iter().chain(runtime_files().iter()) {
        for needle in &stale_calls {
            assert!(
                !contains_outside_comments(path, needle),
                "V2 invariant: {} must not contain `{}` outside comments \
                 (modulation chain deleted; signals flow via sub-KG NodeId)",
                path,
                needle,
            );
        }
    }
}

#[test]
fn no_evidence_context_shadow_path() {
    // BP must have a single entry. The pre-V2 EvidenceBuildContext /
    // EvidenceContext shadow path is forbidden.
    let stale_types = [
        "EvidenceBuildContext",
        "EvidenceContext",
        "WlEvidenceStats",
        "build_inputs_with_evidence",
        "observe_with_evidence",
        "observe_with_evidence_context",
    ];
    for path in pipeline_files().iter().chain(runtime_files().iter()) {
        for needle in &stale_types {
            assert!(
                !contains_outside_comments(path, needle),
                "V2 invariant: {} must not reference `{}` (BP single entry is `build_inputs(registry, edges, lead_lag_events)`; \
                 priors come from sub-KG NodeIds)",
                path,
                needle,
            );
        }
    }
}

#[test]
fn no_external_calibrator_or_backtest() {
    // V2 explicitly rejects external feedback calibrators and external
    // backtest frameworks. Forecast accuracy flows back through
    // NodeId::ForecastAccuracy; rolling validation lives in
    // active_probe (forecast vs reality each tick).
    let banned = [
        "causal_edge_calibrator",
        "backtest_replay",
        "backtest_runner",
    ];
    for path in pipeline_files() {
        for needle in &banned {
            assert!(
                !contains_outside_comments(&path, needle),
                "V2 invariant: {} must not reference `{}` (V2 rejects external feedback paths)",
                path,
                needle,
            );
        }
    }
}

#[test]
fn bp_has_single_public_build_entry() {
    let bp = read("src/pipeline/loopy_bp.rs");
    let stripped = strip_comments(&bp);
    let count = stripped.matches("pub fn build_inputs").count();
    assert_eq!(
        count, 1,
        "V2 invariant: loopy_bp must expose exactly one `pub fn build_inputs` entry, found {count}",
    );
}

#[test]
fn modstage_enum_is_gone() {
    // ModStage was the categorical enumeration of the modulation chain.
    // V2 has no chain to enumerate.
    for path in pipeline_files().iter().chain(runtime_files().iter()) {
        let stripped = strip_comments(&read(path));
        assert!(
            !stripped.contains("enum ModStage"),
            "V2 invariant: {} must not declare `enum ModStage`",
            path,
        );
        assert!(
            !stripped.contains("ModStage::"),
            "V2 invariant: {} must not reference `ModStage::*`",
            path,
        );
    }
}

#[test]
fn active_probe_writes_back_through_nodekind_only() {
    // Active probe must not modify edge weights or call any calibrator
    // function. Its only feedback path is `NodeId::ForecastAccuracy`
    // via `accuracy_by_symbol()` consumed by the substrate builder.
    let probe = strip_comments(&read("src/pipeline/active_probe.rs"));
    assert!(
        !probe.contains("modify_edge_weight"),
        "V2 invariant: active_probe must not modify edge weights",
    );
    assert!(
        !probe.contains("calibrator"),
        "V2 invariant: active_probe must not invoke any calibrator",
    );
    assert!(
        probe.contains("accuracy_by_symbol"),
        "V2 invariant: active_probe must expose `accuracy_by_symbol()` for sub-KG ingestion",
    );
}

#[test]
fn no_hypothesis_id_family_pattern_matching_in_production() {
    // V3 invariant: hypothesis family identification uses the typed
    // `Hypothesis.kind` enum, not `hypothesis_id.contains("hidden_force"
    // /"hidden_connection"/"convergence_hypothesis"/"latent_vortex")`
    // string-pattern matching. Test fixtures may still construct ids
    // with these markers, but production code must dispatch on `kind`.
    let banned = [
        r#"hypothesis_id.contains("hidden_force")"#,
        r#"hypothesis_id.contains("hidden_connection")"#,
        r#"hypothesis_id.contains("convergence_hypothesis")"#,
        r#"hypothesis_id.contains("latent_vortex")"#,
    ];
    for path in pipeline_files().iter().chain(runtime_files().iter()) {
        let stripped = strip_comments(&read(path));
        let prod_only = strip_cfg_test_modules(&stripped);
        for needle in &banned {
            assert!(
                !prod_only.contains(needle),
                "V3 invariant: {} must not contain `{}` outside test fixtures \
                 (use `Hypothesis.kind` enum dispatch)",
                path,
                needle,
            );
        }
    }
}

#[test]
fn substrate_builder_includes_all_v2_node_kinds() {
    // The substrate evidence builder must populate every Phase 1 NodeId.
    let subkg = read("src/pipeline/symbol_sub_kg.rs");
    for nid in [
        "OutcomeMemory",
        "EngramAlignment",
        "WlAnalogConfidence",
        "BeliefEntropy",
        "BeliefSampleCount",
        "ForecastAccuracy",
        "SectorIntentBull",
        "SectorIntentBear",
        // V4 self-referential KL surprise NodeIds — must flow through
        // build_substrate_evidence_snapshots / update_from_substrate_evidence
        // exactly like the other substrate-evidence carriers.
        "KlSurpriseMagnitude",
        "KlSurpriseDirection",
    ] {
        assert!(
            subkg.contains(&format!("NodeId::{nid}")),
            "V2/V4 invariant: NodeId::{nid} must be populated by the substrate builder",
        );
    }
}

#[test]
fn ontology_contract_registry_covers_all_subkg_node_ids() {
    let contract = read("src/pipeline/ontology_contract.rs");
    let subkg = read("src/pipeline/symbol_sub_kg.rs");

    assert!(
        contract.contains("pub fn contract_for(node_id: &NodeId) -> NodeContract"),
        "Ontology contract registry must expose a single contract lookup entry",
    );
    assert!(
        contract.contains("pub fn fixed_node_contracts() -> Vec<NodeContract>"),
        "Ontology contract registry must expose fixed sub-KG template contracts",
    );

    for nid in [
        "OutcomeMemory",
        "EngramAlignment",
        "WlAnalogConfidence",
        "BeliefEntropy",
        "BeliefSampleCount",
        "ForecastAccuracy",
        "SectorIntentBull",
        "SectorIntentBear",
        "KlSurpriseMagnitude",
        "KlSurpriseDirection",
    ] {
        assert!(
            contract.contains(&format!("NodeId::{nid}")),
            "Ontology contract registry must define a contract for NodeId::{nid}",
        );
    }

    for kind in ["Memory", "Belief", "Causal", "Sector", "Surprise"] {
        assert!(
            subkg.contains(&format!("NodeKind::{kind}"))
                && contract.contains(&format!("NodeKind::{kind}")),
            "Ontology contract registry must preserve NodeKind::{kind}",
        );
    }
}

#[test]
fn hk_us_runtime_order_is_symmetric_for_bp_fusion() {
    assert_marker_order(
        "src/hk/runtime.rs",
        &[
            ("regime analog record", "hk_regime_analog_index.record"),
            (
                "substrate evidence build",
                "build_substrate_evidence_snapshots",
            ),
            ("substrate evidence apply", "update_from_substrate_evidence"),
            ("lead-lag edge evidence", "lead_lag_index::detect_lead_lag"),
            ("BP input build", "loopy_bp::build_inputs"),
            ("BP run", "belief_substrate.observe_tick"),
            (
                "posterior confidence",
                "loopy_bp::apply_posterior_confidence",
            ),
            ("active probe evaluate", "hk_active_probe.evaluate_due"),
            ("active probe emit", "hk_active_probe.emit_probes"),
        ],
    );

    assert_marker_order(
        "src/us/runtime.rs",
        &[
            ("regime analog record", "us_regime_analog_index.record"),
            (
                "substrate evidence build",
                "build_substrate_evidence_snapshots",
            ),
            ("substrate evidence apply", "update_from_substrate_evidence"),
            ("lead-lag edge evidence", "lead_lag_index::detect_lead_lag"),
            ("BP input build", "loopy_bp::build_inputs"),
            ("BP run", "belief_substrate.observe_tick"),
            (
                "posterior confidence",
                "loopy_bp::apply_posterior_confidence",
            ),
            ("active probe evaluate", "us_active_probe.evaluate_due"),
            ("active probe emit", "us_active_probe.emit_probes"),
        ],
    );

    let trace = read("src/pipeline/runtime_stage_trace.rs");
    for stage in [
        "RegimeAnalogRecord",
        "SubKgEvidenceBuild",
        "LeadLagDetect",
        "BpBuildInputs",
        "BpRun",
        "BpPosteriorConfidence",
        "ActiveProbeEvaluate",
        "ActiveProbeEmit",
    ] {
        assert!(
            trace.contains(stage),
            "runtime_stage_trace must expose RuntimeStage::{stage}",
        );
    }
}

#[test]
fn us_runtime_reprojects_operator_surface_after_bp_fusion() {
    let runtime = strip_comments(&read("src/us/runtime.rs"));
    let posterior_idx = runtime
        .find("loopy_bp::apply_posterior_confidence")
        .expect("US runtime must apply BP posterior confidence");
    let promotion_idx = runtime
        .find("action_promotion::apply_action_promotion")
        .expect("US runtime must run graph-native action promotion");
    let refresh_idx = runtime
        .find("post_bp_projection_refresh")
        .expect("US runtime must mark post-BP projection refresh");
    let final_projection_idx = runtime
        .rfind("project_us(UsProjectionInputs")
        .expect("US runtime must build a final operator projection");
    let display_idx = runtime
        .rfind("display_us_runtime_summary(")
        .expect("US runtime must display the final operator projection");
    let publish_idx = runtime
        .rfind("run_us_projection_stage(")
        .expect("US runtime must publish the final operator projection");

    assert!(
        posterior_idx < promotion_idx,
        "US BP posterior confidence must precede action promotion"
    );
    assert!(
        promotion_idx < refresh_idx,
        "US action promotion must mark the operator surface dirty"
    );
    assert!(
        refresh_idx < final_projection_idx,
        "US runtime must re-run project_us after BP/action promotion"
    );
    assert!(
        final_projection_idx < display_idx,
        "US console display must consume the refreshed projection"
    );
    assert!(
        display_idx < publish_idx,
        "US persistence publish must consume the refreshed projection"
    );
}

#[test]
fn bp_message_trace_exports_priors_messages_and_posteriors() {
    let bp = read("src/pipeline/loopy_bp.rs");
    for needle in [
        "pub struct BpMessageTraceRow",
        "pub fn run_with_messages",
        "pub fn build_message_trace_rows",
        "pub fn write_message_trace",
        "BpTraceKind::Prior",
        "BpTraceKind::Message",
        "BpTraceKind::Posterior",
    ] {
        assert!(
            bp.contains(needle),
            "BP message trace artifact must expose `{needle}`",
        );
    }

    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = read(path);
        assert!(
            runtime.contains("build_belief_only_trace_rows")
                && runtime.contains("write_message_trace"),
            "{path} must write BP belief trace rows alongside BP marginals",
        );
    }
}

#[test]
fn encoded_tick_frame_contract_exists_for_v7_encoder_pass() {
    let module = read("src/pipeline/mod.rs");
    let frame = read("src/pipeline/encoded_tick_frame.rs");

    assert!(
        module.contains("pub mod encoded_tick_frame"),
        "V7 invariant: pipeline must expose encoded_tick_frame as the shared perception contract",
    );

    for needle in [
        "pub struct EncodedTickFrame",
        "pub struct EncodedSymbolFrame",
        "pub scale: TimeScale",
        "pub channel: PressureChannel",
        "pub const ENCODED_TICK_FRAME_VERSION",
        "pub fn from_pressure_field",
        "pub fn attach_subkg_registry",
        "pub fn attach_bp_state",
        "pub fn write_frame",
        "RuntimeArtifactKind::EncodedTickFrame",
        "PressureField",
        "SubKgRegistry",
        "NodePrior",
        "GraphEdge",
    ] {
        assert!(
            frame.contains(needle),
            "V7 invariant: EncodedTickFrame must expose `{needle}` for encode-once/decode-many migration",
        );
    }
}

#[test]
fn visual_graph_frame_exports_subkg_master_edges_and_bp_state() {
    let frame = read("src/pipeline/visual_graph_frame.rs");
    for needle in [
        "pub struct VisualGraphFrame",
        "pub struct VisualSubKgNode",
        "pub struct VisualMasterEdge",
        "pub fn build_visual_graph_frame",
        "pub fn build_visual_graph_frame_from_encoded",
        "pub fn write_frame",
        "EncodedTickFrame",
        "visual_frame_parity_raw_vs_encoded",
        "SubKgRegistry",
        "GraphEdge",
        "NodePrior",
    ] {
        assert!(
            frame.contains(needle),
            "visual graph frame backend must expose `{needle}`",
        );
    }

    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = read(path);
        assert!(
            runtime.contains("build_visual_graph_frame") && runtime.contains("write_frame"),
            "{path} must write VisualGraphFrame from the same BP pass",
        );
    }
}

#[test]
fn hk_runtime_builds_encoded_tick_frame_before_visual_decode() {
    let runtime = read("src/hk/runtime.rs");
    assert!(
        runtime.contains("EncodedTickFrame::from_pressure_field"),
        "HK runtime must build the V7 encoded tick frame after pressure/BP state exists",
    );
    assert!(
        runtime.contains("encoded_tick_frame.attach_subkg_registry")
            && runtime.contains("encoded_tick_frame.attach_bp_state"),
        "HK runtime must attach sub-KG and BP state to the same encoded frame",
    );
    assert!(
        runtime.contains("encoded_tick_frame::write_frame"),
        "HK runtime must persist the encoded frame artifact for auditability",
    );
    assert!(
        runtime.contains("build_visual_graph_frame_from_encoded"),
        "HK visual graph frame must decode from EncodedTickFrame, not direct raw runtime state",
    );
}

#[test]
fn v7_reboot_keeps_graph_substrate_instead_of_flat_tick_frame() {
    let pipeline_mod = read("src/pipeline/mod.rs");
    assert!(
        !pipeline_mod.contains("pub mod tick_frame;"),
        "V7 reboot must not reintroduce a flat TickFrame dict artifact; use encoded_tick_frame + graph substrate",
    );
    assert!(
        pipeline_mod.contains("pub mod encoded_tick_frame;"),
        "V7 reboot must expose encoded_tick_frame as the shared perception contract",
    );

    let subkg = read("src/pipeline/symbol_sub_kg.rs");
    for needle in [
        "pub struct Edge",
        "pub enum EdgeKind",
        "pub struct SymbolSubKG",
        "pub edges: Vec<Edge>",
        "Contributes",
        "Evidence",
        "IntentToState",
    ] {
        assert!(
            subkg.contains(needle),
            "graph-native V7 substrate must preserve sub-KG graph primitive `{needle}`",
        );
    }

    let visual = read("src/pipeline/visual_graph_frame.rs");
    for needle in [
        "pub struct VisualSubKgEdge",
        "pub struct VisualMasterEdge",
        "pub master_edges: Vec<VisualMasterEdge>",
        "visual_edges",
    ] {
        assert!(
            visual.contains(needle),
            "graph-native V7 inspection surface must preserve graph primitive `{needle}`",
        );
    }

    let bp = read("src/pipeline/loopy_bp.rs");
    for needle in [
        "pub struct GraphEdge",
        "pub fn observe_from_subkg",
        "pub fn run_with_messages",
        "directed_edge_weights",
        "CONVERGENCE_TOL",
    ] {
        assert!(
            bp.contains(needle),
            "graph-native V7 propagation must build on BP primitive `{needle}`",
        );
    }
}

#[test]
fn runtime_health_tick_records_bp_probe_and_artifact_write_health_for_both_markets() {
    let artifacts = read("src/core/runtime_artifacts.rs");
    for needle in [
        "pub struct RuntimeHealthTick",
        "bp_iterations",
        "bp_converged",
        "bp_master_graph_edges",
        "bp_master_runtime_edges",
        "bp_build_inputs_ms",
        "bp_run_ms",
        "bp_message_trace_write_ms",
        "bp_marginals_write_ms",
        "bp_shadow_observed_incident_edges",
        "bp_shadow_low_weight_edges",
        "bp_shadow_retained_edges",
        "bp_shadow_pruned_edges",
        "bp_shadow_stock_to_stock_edges",
        "bp_shadow_unknown_edges",
        "stage_plan_expected_count",
        "stage_plan_covered",
        "probe_pending",
        "artifact_write_errors",
        "RuntimeArtifactKind::RuntimeHealthTick",
        "pub fn write_runtime_health_tick",
        "frontier_symbols",
        "frontier_nodes",
        "frontier_edges",
        "frontier_hops",
        "frontier_candidates",
        "frontier_dry_run_updates",
        "frontier_dry_run_mean_abs_delta",
        "frontier_dry_run_max_abs_delta",
        "frontier_pressure_cache_updates",
        "frontier_pressure_cache_mean_abs_delta",
        "frontier_pressure_cache_max_abs_delta",
        "frontier_pressure_gate_passed",
        "frontier_pressure_gate_noise_floor",
        "frontier_next_proposals",
        "frontier_loop_rounds",
        "frontier_loop_final_proposals",
    ] {
        assert!(
            artifacts.contains(needle),
            "runtime artifact layer must expose `{needle}`",
        );
    }

    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = read(path);
        assert!(
            runtime.contains("RuntimeHealthTick")
                && runtime.contains("write_runtime_health_tick")
                && runtime.contains("record_artifact_result"),
            "{path} must write RuntimeHealthTick from the same BP/probe cycle",
        );
        assert!(
            runtime.contains("bp_build_inputs_elapsed")
                && runtime.contains("bp_run_elapsed")
                && runtime.contains("bp_message_trace_write_elapsed")
                && runtime.contains("bp_marginals_write_elapsed"),
            "{path} must time BP build/run/write stages separately",
        );
        assert!(
            runtime.contains("BpInputEdge")
                && runtime.contains("loopy_bp::build_inputs")
                && runtime.contains("build_pruning_shadow_summary")
                && runtime.contains("bp_shadow_pruned_edges"),
            "{path} must preserve typed BP edges and expose pruning shadow metrics",
        );
    }
}

#[test]
fn runtime_uses_directed_master_edges_without_synthesizing_reverse_pairs() {
    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = strip_comments(&read(path));
        assert!(
            runtime.contains("edge_endpoints")
                && runtime.contains("edge_type: \"StockToStock\".into()"),
            "{path} must build BP master edges from graph edge endpoints",
        );
        assert!(
            !runtime.contains("to: a,") && !runtime.contains("to: sa,"),
            "{path} must not synthesize reverse StockToStock master edges; BrainGraph/UsGraph already own directed topology",
        );
    }
}

#[test]
fn runtime_stage_plan_is_the_shared_hk_us_contract() {
    let trace = read("src/pipeline/runtime_stage_trace.rs");
    for needle in [
        "pub struct RuntimeStagePlan",
        "subkg_bp_probe_artifact_health",
        "RuntimeStage::SubKgEvidenceBuild",
        "RuntimeStage::SectorSubKgBuild",
        "RuntimeStage::FrontierBuild",
        "RuntimeStage::CrossSymbolPropagation",
        "RuntimeStage::LeadLagDetect",
        "RuntimeStage::BpBuildInputs",
        "RuntimeStage::ActiveProbeEvaluate",
        "RuntimeStage::ArtifactHealth",
        "RuntimeStage::ActiveProbeEmit, RuntimeStage::ArtifactHealth",
    ] {
        assert!(
            trace.contains(needle),
            "runtime stage plan must declare `{needle}`",
        );
    }

    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = read(path);
        assert!(
            runtime.contains("RuntimeStagePlan::canonical()")
                && runtime.contains("record_planned(stage_plan")
                && runtime.contains("plan_coverage(stage_plan"),
            "{path} must record stages through the shared RuntimeStagePlan",
        );
        assert_marker_order(
            path,
            &[
                ("artifact health stage", "RuntimeStage::ArtifactHealth"),
                ("stage trace artifact", "runtime_trace.write_ndjson()"),
                ("runtime health artifact", "write_runtime_health_tick"),
            ],
        );
        assert_marker_order(
            path,
            &[
                (
                    "sub-KG evidence applied",
                    "RuntimeStage::SubKgEvidenceApply",
                ),
                ("graph frontier built", "RuntimeStage::FrontierBuild"),
                ("sub-KG snapshot write", "RuntimeStage::SubKgSnapshotWrite"),
            ],
        );
        assert!(
            runtime.contains("GraphFrontier::from_subkg_registry")
                && runtime.contains("frontier_symbols")
                && runtime.contains("frontier_nodes")
                && runtime.contains("frontier_edges")
                && runtime.contains("frontier_hops")
                && runtime.contains("frontier_candidates")
                && runtime.contains("frontier_dry_run_updates")
                && runtime.contains("frontier_dry_run_mean_abs_delta")
                && runtime.contains("frontier_dry_run_max_abs_delta")
                && runtime.contains("frontier_pressure_cache_updates")
                && runtime.contains("frontier_pressure_cache_mean_abs_delta")
                && runtime.contains("frontier_pressure_cache_max_abs_delta")
                && runtime.contains("frontier_pressure_gate_passed")
                && runtime.contains("frontier_pressure_gate_noise_floor")
                && runtime.contains("frontier_next_proposals")
                && runtime.contains("frontier_loop_rounds")
                && runtime.contains("frontier_loop_final_proposals")
                && runtime.contains("local_propagation_plan"),
            "{path} must expose graph frontier counts in runtime health",
        );
    }
}

#[test]
fn hk_us_runtime_stage_timer_markers_stay_symmetric() {
    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = read(path);
        for needle in [
            "TickStageTimer::new()",
            "S01_trade_tape_feed",
            "S02_S03_canonical",
            "S04_S06_perception_pressure",
            "S07_S13_setups_bp_hub",
            "S18_signal_momentum_feed",
            "S14_S19_state_workflow_projection",
            "S20_wake_surface",
            "S21a_sk_snapshots",
            "S21c_heartbeat_tail",
            "stage_top5_ms",
        ] {
            assert!(
                runtime.contains(needle),
                "{path} must keep runtime stage timer marker `{needle}`",
            );
        }
    }

    let hk_persistence = read("src/hk/runtime/persistence.rs");
    let us_persistence = read("src/us/runtime/support/stages.rs");
    for (path, src) in [
        ("src/hk/runtime/persistence.rs", hk_persistence),
        ("src/us/runtime/support/stages.rs", us_persistence),
    ] {
        for needle in [
            "S21b2_outcomes_compute",
            "S21b4_settle_horizons",
            "S21b5_persist_perception_states",
        ] {
            assert!(
                src.contains(needle),
                "{path} must keep projection persistence timer marker `{needle}`",
            );
        }
    }
}

#[test]
fn persistence_breakdown_scaffold_stays_removed() {
    let context = strip_comments(&read("src/core/runtime/context.rs"));
    for banned in [
        "EDEN_PERSIST_TIMING",
        "persist_timing_enabled",
        "persist_followups_breakdown",
        "persist_followups_outer",
        "S21b3a_publish_projection",
        "S21b3b_persist_followups",
    ] {
        assert!(
            !context.contains(banned),
            "diagnostic persistence scaffold `{banned}` should stay removed",
        );
    }
}

#[test]
fn frontier_worker_is_graph_local_not_registry_scan() {
    let frontier = read("src/pipeline/frontier.rs");
    for needle in [
        "pub struct FrontierPropagationPlan",
        "pub struct FrontierPropagationHop",
        "pub struct FrontierPropagationCandidate",
        "pub struct FrontierPropagationDryRun",
        "pub struct FrontierDryRunUpdate",
        "pub struct FrontierPressureCandidateCache",
        "pub struct FrontierPressureCandidateUpdate",
        "pub struct FrontierPressureConvergenceGate",
        "pub struct FrontierNextProposal",
        "pub struct FrontierNextProposalEntry",
        "pub struct FrontierBoundedPropagationSummary",
        "pub struct FrontierPropagationRoundSummary",
        "pub fn local_propagation_plan",
        "pub fn bounded_propagation_summary",
        "propagation_candidates",
        "from_candidates",
        "from_dry_run",
        "is_pressure_candidate_edge",
        "is_pressure_target",
        "is_directional_propagation_edge",
        "from_pressure_gate",
        "edge.from == node.id",
        "expand_proposals_once",
    ] {
        assert!(
            frontier.contains(needle),
            "frontier worker must expose graph-local primitive `{needle}`",
        );
    }

    let plan_body_start = frontier
        .find("pub fn local_propagation_plan")
        .expect("local propagation plan exists");
    let test_section_start = frontier
        .find("#[cfg(test)]")
        .expect("frontier tests section exists");
    let plan_body = &frontier[plan_body_start..test_section_start];
    assert!(
        !plan_body.contains("SubKgRegistry"),
        "local propagation plan must consume frontier edges, not rescan SubKgRegistry",
    );
    assert!(
        !plan_body.contains("graphs.iter()"),
        "local propagation plan must not fall back to full registry scans",
    );
}

#[test]
fn graph_query_backend_reads_visual_frame_without_new_inference_path() {
    let query = read("src/pipeline/graph_query_backend.rs");
    for needle in [
        "pub struct GraphQueryBackend",
        "pub fn ego",
        "pub fn nodes_by_kind",
        "pub fn influence",
        "pub fn ranked_probe_accuracy",
        "VisualGraphFrame",
        "NodeKind",
    ] {
        assert!(
            query.contains(needle),
            "graph query backend must expose `{needle}`",
        );
    }
    for forbidden in [
        "observe_from_subkg(",
        "loopy_bp::run",
        "build_inputs(",
        "apply_posterior_confidence",
    ] {
        assert!(
            !query.contains(forbidden),
            "graph query backend must not create a second inference path via `{forbidden}`",
        );
    }
}

#[test]
fn temporal_graph_delta_tracks_drift_from_visual_frames_only() {
    let delta = read("src/pipeline/temporal_graph_delta.rs");
    for needle in [
        "pub struct TemporalGraphDelta",
        "pub struct NodeDelta",
        "pub struct SubKgEdgeDelta",
        "pub struct MasterEdgeDelta",
        "pub struct PosteriorDelta",
        "pub fn build_delta",
        "pub fn write_delta",
        "VisualGraphFrame",
    ] {
        assert!(
            delta.contains(needle),
            "temporal graph delta must expose `{needle}`",
        );
    }
    for forbidden in [
        "observe_from_subkg(",
        "loopy_bp::run",
        "build_inputs(",
        "apply_posterior_confidence",
    ] {
        assert!(
            !delta.contains(forbidden),
            "temporal graph delta must not create a second inference path via `{forbidden}`",
        );
    }

    for path in ["src/hk/runtime.rs", "src/us/runtime.rs"] {
        let runtime = read(path);
        assert!(
            runtime.contains("previous_visual_frame")
                && runtime.contains("temporal_graph_delta::build_delta")
                && runtime.contains("temporal_graph_delta::write_delta"),
            "{path} must emit temporal graph deltas from consecutive VisualGraphFrame snapshots",
        );
    }
}
