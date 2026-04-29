#[cfg(feature = "persistence")]
use super::builders::ordered_unique;
#[cfg(feature = "persistence")]
use crate::live_snapshot::LiveMarket;
#[cfg(feature = "persistence")]
use crate::math::clamp_unit_interval;
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
#[cfg(feature = "persistence")]
use rust_decimal::Decimal;
#[cfg(feature = "persistence")]
use std::collections::HashMap;
#[cfg(feature = "persistence")]
use time::OffsetDateTime;

use super::types::CaseReasoningAssessmentSnapshot;
#[cfg(feature = "persistence")]
use super::types::{
    CaseDetail, CaseInvalidationPatternStat, CaseMechanismStory, CaseMechanismTransition,
    CaseMechanismTransitionDigest, CaseMechanismTransitionSliceStat, CaseMechanismTransitionStat,
    CaseSummary,
};

#[path = "reasoning_story/analytics.rs"]
mod analytics;
#[path = "reasoning_story/shared.rs"]
mod shared;
#[path = "reasoning_story/story.rs"]
mod story;

#[cfg(feature = "persistence")]
pub(in crate::cases) use analytics::{
    build_invalidation_patterns, build_mechanism_transition_analytics,
};
#[cfg(feature = "persistence")]
pub(in crate::cases) use shared::record_invalidation_rules;
#[cfg(feature = "persistence")]
pub(in crate::cases) use story::build_case_mechanism_story;
#[cfg(feature = "persistence")]
#[allow(unused_imports)]
pub(in crate::cases) use story::describe_mechanism_transition;
