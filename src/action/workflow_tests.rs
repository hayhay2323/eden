    use super::*;
    use serde_json::json;

    fn ts(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).expect("valid timestamp")
    }

    #[test]
    fn stage_progression_is_linear() {
        assert_eq!(ActionStage::Suggest.next(), Some(ActionStage::Confirm));
        assert_eq!(ActionStage::Confirm.next(), Some(ActionStage::Execute));
        assert_eq!(ActionStage::Execute.next(), Some(ActionStage::Monitor));
        assert_eq!(ActionStage::Monitor.next(), Some(ActionStage::Review));
        assert_eq!(ActionStage::Review.next(), None);
    }

    #[test]
    fn state_transitions_preserve_descriptor() {
        let descriptor = ActionDescriptor::new("wf-1", "Test action", json!({"kind": "demo"}));
        let suggested = SuggestedAction::new(
            descriptor,
            ts(1_773_914_400),
            Some("system".to_string()),
            Some("initial".to_string()),
        );

        let confirmed = suggested.confirm(
            ts(1_773_914_460),
            Some("ops".to_string()),
            Some("approved".to_string()),
        );

        assert_eq!(confirmed.descriptor.workflow_id, "wf-1");
        assert_eq!(confirmed.descriptor.title, "Test action");
        assert_eq!(confirmed.stage(), ActionStage::Confirm);

        let snapshot = ActionWorkflowSnapshot::from_state(&confirmed);
        assert_eq!(snapshot.stage, ActionStage::Confirm);
        assert_eq!(snapshot.actor.as_deref(), Some("ops"));
    }

    #[test]
    fn governance_contract_exposes_allowed_transitions() {
        let contract = workflow_governance(Some(ActionStage::Suggest));
        assert_eq!(contract.current_stage, Some(ActionStage::Suggest));
        assert_eq!(
            contract.allowed_transitions,
            vec![ActionStage::Confirm, ActionStage::Review]
        );
        assert!(contract.human_override_supported);
        assert!(!contract.assignment_locked);
    }

    #[test]
    fn validate_transition_rejects_non_linear_jump_except_review() {
        assert!(validate_transition(Some(ActionStage::Suggest), ActionStage::Confirm).is_ok());
        assert!(validate_transition(Some(ActionStage::Suggest), ActionStage::Review).is_ok());
        assert!(validate_transition(Some(ActionStage::Suggest), ActionStage::Execute).is_err());
    }

    #[test]
    fn assignment_locks_during_execute_stage() {
        assert!(validate_assignment_update(Some(ActionStage::Suggest)).is_ok());
        assert!(validate_assignment_update(Some(ActionStage::Execute)).is_err());
    }

    #[test]
    fn governance_reason_explains_execute_lock() {
        let reason = governance_reason(
            Some(ActionStage::Execute),
            ActionExecutionPolicy::ReviewRequired,
        );
        assert!(reason.contains("assignment is locked"));
        assert!(reason.contains("review_required"));
    }

    #[test]
    fn queue_pin_cannot_be_reassigned_by_another_actor() {
        let error = validate_queue_pin_update(
            Some("frontend-review-list"),
            Some(&Some("ops-desk".into())),
            Some("frontend"),
        )
        .expect_err("different actor should not reassign queue pin");
        assert!(error
            .to_string()
            .contains("queue pin is owned by `frontend-review-list`"));
    }

    #[test]
    fn queue_pin_can_be_cleared_by_current_owner() {
        assert!(validate_queue_pin_update(
            Some("frontend-review-list"),
            Some(&None),
            Some("frontend-review-list"),
        )
        .is_ok());
    }
