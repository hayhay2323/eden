//! Runtime stage trace for the sub-KG -> BP fusion path.
//!
//! This is observability only. It does not feed inference and does not
//! alter priors, edges, or setup confidence. The trace exists to make the
//! HK/US ordering contract inspectable: substrate evidence must be in the
//! sub-KG before BP inputs are built, and posterior confidence must come
//! from the BP run before active probes are emitted.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::core::market::MarketRegistry;
use crate::core::runtime_artifacts::{RuntimeArtifactKind, RuntimeArtifactStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStage {
    RegimeAnalogRecord,
    WlAnalogRecord,
    ActiveProbeAccuracyRead,
    SubKgEvidenceBuild,
    SubKgEvidenceApply,
    FrontierBuild,
    SubKgSnapshotWrite,
    SectorSubKgBuild,
    CrossSymbolPropagation,
    LeadLagDetect,
    BpBuildInputs,
    BpRun,
    BpMarginalsWrite,
    BpPosteriorConfidence,
    ActiveProbeEvaluate,
    ActiveProbeEmit,
    ArtifactHealth,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStageRecord {
    pub ordinal: usize,
    pub stage: RuntimeStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStageTrace {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick: u64,
    pub stages: Vec<RuntimeStageRecord>,
}

impl RuntimeStageTrace {
    pub fn new(market: impl Into<String>, tick: u64, ts: DateTime<Utc>) -> Self {
        Self {
            ts,
            market: market.into(),
            tick,
            stages: Vec::new(),
        }
    }

    pub fn record(&mut self, stage: RuntimeStage) {
        self.record_detail(stage, None::<String>);
    }

    pub fn record_planned(
        &mut self,
        plan: RuntimeStagePlan<'_>,
        stage: RuntimeStage,
    ) -> Result<(), RuntimeStagePlanError> {
        if !plan.contains(stage) {
            return Err(RuntimeStagePlanError {
                plan: plan.name().to_string(),
                stage,
            });
        }
        self.record(stage);
        Ok(())
    }

    pub fn record_detail(&mut self, stage: RuntimeStage, detail: impl Into<Option<String>>) {
        self.stages.push(RuntimeStageRecord {
            ordinal: self.stages.len(),
            stage,
            detail: detail.into(),
        });
    }

    pub fn validate_bp_order(&self) -> Result<(), RuntimeStageOrderError> {
        validate_bp_order(self.stages.iter().map(|r| r.stage))
    }

    pub fn plan_coverage(&self, plan: RuntimeStagePlan<'_>) -> RuntimeStagePlanCoverage {
        let recorded: Vec<RuntimeStage> = self.stages.iter().map(|r| r.stage).collect();
        RuntimeStagePlanCoverage {
            plan: plan.name().to_string(),
            expected_stage_count: plan.expected_stage_count(),
            recorded_stage_count: recorded
                .iter()
                .filter(|stage| plan.contains(**stage))
                .count(),
            covered: plan.stages().iter().all(|stage| recorded.contains(stage)),
        }
    }

    pub fn covers_plan(&self, plan: RuntimeStagePlan<'_>) -> bool {
        self.plan_coverage(plan).covered
    }

    pub fn write_ndjson(&self) -> std::io::Result<usize> {
        if let Err(err) = self.validate_bp_order() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            ));
        }
        let market = MarketRegistry::by_slug(&self.market).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown market for runtime stage trace: {}", self.market),
            )
        })?;
        RuntimeArtifactStore::default().append_json_line(
            RuntimeArtifactKind::RuntimeStageTrace,
            market,
            self,
        )?;
        Ok(1)
    }
}

const CANONICAL_RUNTIME_STAGE_PLAN: &[RuntimeStage] = &[
    RuntimeStage::RegimeAnalogRecord,
    RuntimeStage::WlAnalogRecord,
    RuntimeStage::ActiveProbeAccuracyRead,
    RuntimeStage::SubKgEvidenceBuild,
    RuntimeStage::SubKgEvidenceApply,
    RuntimeStage::FrontierBuild,
    RuntimeStage::SubKgSnapshotWrite,
    RuntimeStage::SectorSubKgBuild,
    RuntimeStage::CrossSymbolPropagation,
    RuntimeStage::LeadLagDetect,
    RuntimeStage::BpBuildInputs,
    RuntimeStage::BpRun,
    RuntimeStage::BpMarginalsWrite,
    RuntimeStage::BpPosteriorConfidence,
    RuntimeStage::ActiveProbeEvaluate,
    RuntimeStage::ActiveProbeEmit,
    RuntimeStage::ArtifactHealth,
];

#[derive(Debug, Clone, Copy)]
pub struct RuntimeStagePlan<'a> {
    name: &'static str,
    stages: &'a [RuntimeStage],
}

impl RuntimeStagePlan<'static> {
    pub fn canonical() -> Self {
        Self {
            name: "subkg_bp_probe_artifact_health",
            stages: CANONICAL_RUNTIME_STAGE_PLAN,
        }
    }
}

impl<'a> RuntimeStagePlan<'a> {
    #[cfg(test)]
    fn without_for_test(stages: &'a [RuntimeStage]) -> Self {
        Self {
            name: "test",
            stages,
        }
    }

    pub fn name(self) -> &'static str {
        self.name
    }

    pub fn stages(self) -> &'a [RuntimeStage] {
        self.stages
    }

    pub fn expected_stage_count(self) -> usize {
        self.stages.len()
    }

    pub fn contains(self, stage: RuntimeStage) -> bool {
        self.stages.contains(&stage)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStagePlanCoverage {
    pub plan: String,
    pub expected_stage_count: usize,
    pub recorded_stage_count: usize,
    pub covered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStagePlanError {
    pub plan: String,
    pub stage: RuntimeStage,
}

impl fmt::Display for RuntimeStagePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "runtime stage {:?} is not declared in plan {}",
            self.stage, self.plan
        )
    }
}

impl std::error::Error for RuntimeStagePlanError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStageOrderError {
    pub before: RuntimeStage,
    pub after: RuntimeStage,
    pub before_index: Option<usize>,
    pub after_index: Option<usize>,
}

impl fmt::Display for RuntimeStageOrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "runtime stage order violation: {:?} at {:?} must precede {:?} at {:?}",
            self.before, self.before_index, self.after, self.after_index,
        )
    }
}

impl std::error::Error for RuntimeStageOrderError {}

pub fn validate_bp_order<I>(stages: I) -> Result<(), RuntimeStageOrderError>
where
    I: IntoIterator<Item = RuntimeStage>,
{
    let ordered: Vec<RuntimeStage> = stages.into_iter().collect();
    for (before, after) in [
        (
            RuntimeStage::RegimeAnalogRecord,
            RuntimeStage::SubKgEvidenceBuild,
        ),
        (
            RuntimeStage::WlAnalogRecord,
            RuntimeStage::SubKgEvidenceBuild,
        ),
        (
            RuntimeStage::ActiveProbeAccuracyRead,
            RuntimeStage::SubKgEvidenceBuild,
        ),
        (
            RuntimeStage::SubKgEvidenceBuild,
            RuntimeStage::SubKgEvidenceApply,
        ),
        (
            RuntimeStage::SubKgEvidenceApply,
            RuntimeStage::FrontierBuild,
        ),
        (
            RuntimeStage::FrontierBuild,
            RuntimeStage::SubKgSnapshotWrite,
        ),
        (
            RuntimeStage::SubKgEvidenceApply,
            RuntimeStage::BpBuildInputs,
        ),
        (
            RuntimeStage::SectorSubKgBuild,
            RuntimeStage::CrossSymbolPropagation,
        ),
        (
            RuntimeStage::CrossSymbolPropagation,
            RuntimeStage::LeadLagDetect,
        ),
        (RuntimeStage::LeadLagDetect, RuntimeStage::BpBuildInputs),
        (RuntimeStage::BpBuildInputs, RuntimeStage::BpRun),
        (RuntimeStage::BpRun, RuntimeStage::BpMarginalsWrite),
        (RuntimeStage::BpRun, RuntimeStage::BpPosteriorConfidence),
        (RuntimeStage::BpRun, RuntimeStage::ActiveProbeEvaluate),
        (RuntimeStage::BpBuildInputs, RuntimeStage::ActiveProbeEmit),
        (
            RuntimeStage::ActiveProbeEvaluate,
            RuntimeStage::ActiveProbeEmit,
        ),
        (RuntimeStage::ActiveProbeEmit, RuntimeStage::ArtifactHealth),
    ] {
        ensure_before(&ordered, before, after)?;
    }
    Ok(())
}

fn ensure_before(
    stages: &[RuntimeStage],
    before: RuntimeStage,
    after: RuntimeStage,
) -> Result<(), RuntimeStageOrderError> {
    let before_index = stages.iter().position(|s| *s == before);
    let after_index = stages.iter().position(|s| *s == after);
    if let (Some(before_index), Some(after_index)) = (before_index, after_index) {
        if before_index > after_index {
            return Err(RuntimeStageOrderError {
                before,
                after,
                before_index: Some(before_index),
                after_index: Some(after_index),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_hk_us_bp_order() {
        let stages = RuntimeStagePlan::canonical().stages();

        assert!(validate_bp_order(stages.iter().copied()).is_ok());
    }

    #[test]
    fn canonical_stage_plan_declares_runtime_contract() {
        let plan = RuntimeStagePlan::canonical();
        let stages = plan.stages();

        assert_eq!(plan.name(), "subkg_bp_probe_artifact_health");
        assert_eq!(plan.expected_stage_count(), stages.len());
        assert_eq!(
            stages,
            &[
                RuntimeStage::RegimeAnalogRecord,
                RuntimeStage::WlAnalogRecord,
                RuntimeStage::ActiveProbeAccuracyRead,
                RuntimeStage::SubKgEvidenceBuild,
                RuntimeStage::SubKgEvidenceApply,
                RuntimeStage::FrontierBuild,
                RuntimeStage::SubKgSnapshotWrite,
                RuntimeStage::SectorSubKgBuild,
                RuntimeStage::CrossSymbolPropagation,
                RuntimeStage::LeadLagDetect,
                RuntimeStage::BpBuildInputs,
                RuntimeStage::BpRun,
                RuntimeStage::BpMarginalsWrite,
                RuntimeStage::BpPosteriorConfidence,
                RuntimeStage::ActiveProbeEvaluate,
                RuntimeStage::ActiveProbeEmit,
                RuntimeStage::ArtifactHealth,
            ]
        );
    }

    #[test]
    fn trace_reports_canonical_plan_coverage() {
        let mut trace = RuntimeStageTrace::new("hk", 7, Utc::now());
        let plan = RuntimeStagePlan::canonical();
        for stage in plan.stages() {
            trace.record_planned(plan, *stage).expect("planned stage");
        }

        assert!(trace.covers_plan(plan));
        assert_eq!(trace.plan_coverage(plan).expected_stage_count, 17);
        assert_eq!(trace.plan_coverage(plan).recorded_stage_count, 17);
    }

    #[test]
    fn rejects_artifact_health_before_probe_emit() {
        let err = validate_bp_order([RuntimeStage::ArtifactHealth, RuntimeStage::ActiveProbeEmit])
            .unwrap_err();

        assert_eq!(err.before, RuntimeStage::ActiveProbeEmit);
        assert_eq!(err.after, RuntimeStage::ArtifactHealth);
    }

    #[test]
    fn rejects_unplanned_stage_recording() {
        let plan = RuntimeStagePlan::without_for_test(&[RuntimeStage::SubKgEvidenceBuild]);
        let mut trace = RuntimeStageTrace::new("hk", 7, Utc::now());
        let err = trace
            .record_planned(plan, RuntimeStage::BpRun)
            .expect_err("stage is not in plan");

        assert_eq!(err.stage, RuntimeStage::BpRun);
    }

    #[test]
    fn accepts_legacy_explicit_hk_us_bp_order() {
        let stages = [
            RuntimeStage::RegimeAnalogRecord,
            RuntimeStage::WlAnalogRecord,
            RuntimeStage::ActiveProbeAccuracyRead,
            RuntimeStage::SubKgEvidenceBuild,
            RuntimeStage::SubKgEvidenceApply,
            RuntimeStage::FrontierBuild,
            RuntimeStage::SubKgSnapshotWrite,
            RuntimeStage::SectorSubKgBuild,
            RuntimeStage::CrossSymbolPropagation,
            RuntimeStage::LeadLagDetect,
            RuntimeStage::BpBuildInputs,
            RuntimeStage::BpRun,
            RuntimeStage::BpMarginalsWrite,
            RuntimeStage::BpPosteriorConfidence,
            RuntimeStage::ActiveProbeEvaluate,
            RuntimeStage::ActiveProbeEmit,
            RuntimeStage::ArtifactHealth,
        ];

        assert!(validate_bp_order(stages).is_ok());
    }

    #[test]
    fn rejects_bp_before_subkg_evidence() {
        let err = validate_bp_order([
            RuntimeStage::BpBuildInputs,
            RuntimeStage::SubKgEvidenceApply,
        ])
        .unwrap_err();

        assert_eq!(err.before, RuntimeStage::SubKgEvidenceApply);
        assert_eq!(err.after, RuntimeStage::BpBuildInputs);
    }

    #[test]
    fn rejects_probe_emit_before_evaluate() {
        let err = validate_bp_order([
            RuntimeStage::BpBuildInputs,
            RuntimeStage::BpRun,
            RuntimeStage::ActiveProbeEmit,
            RuntimeStage::ActiveProbeEvaluate,
        ])
        .unwrap_err();

        assert_eq!(err.before, RuntimeStage::ActiveProbeEvaluate);
        assert_eq!(err.after, RuntimeStage::ActiveProbeEmit);
    }

    #[test]
    fn rejects_cross_symbol_before_sector_subkg_build() {
        let err = validate_bp_order([
            RuntimeStage::CrossSymbolPropagation,
            RuntimeStage::SectorSubKgBuild,
        ])
        .unwrap_err();

        assert_eq!(err.before, RuntimeStage::SectorSubKgBuild);
        assert_eq!(err.after, RuntimeStage::CrossSymbolPropagation);
    }
}
