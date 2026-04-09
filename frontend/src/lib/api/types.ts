export type EdenMarket = "hk" | "us";

export interface ContextStatus {
  runtime_features: string[];
  context_layers_available: boolean;
  coordinator_available: boolean;
  task_lifecycle_available: boolean;
  tool_registry_available: boolean;
}

export interface OperationalObjectRef {
  id: string;
  kind:
    | "market_session"
    | "symbol_state"
    | "case"
    | "recommendation"
    | "macro_event"
    | "thread"
    | "workflow";
  endpoint: string;
  label?: string | null;
}

export interface OperationalGraphRef {
  node_id: string;
  node_kind: string;
  endpoint: string;
}

export interface OperationalGraphNodeStateRecord {
  state_id: string;
  node_id: string;
  node_kind: string;
  label: string;
  market: string;
  latest_tick_number: number;
  last_seen_at: string;
  attributes: unknown;
}

export interface OperationalGraphLinkStateRecord {
  state_id: string;
  link_id: string;
  market: string;
  latest_tick_number: number;
  last_seen_at: string;
  relation: string;
  source_node_kind: string;
  source_node_id: string;
  source_label: string;
  target_node_kind: string;
  target_node_id: string;
  target_label: string;
  confidence: number | string;
  attributes: unknown;
  rationale?: string | null;
}

export interface OperationalGraphEventStateRecord {
  state_id: string;
  event_id: string;
  market: string;
  latest_tick_number: number;
  last_seen_at: string;
  kind: string;
  subject_node_kind: string;
  subject_node_id: string;
  subject_label: string;
  object_node_kind?: string | null;
  object_node_id?: string | null;
  object_label?: string | null;
  confidence: number | string;
  evidence: unknown[];
  attributes: unknown;
  rationale?: string | null;
}

export interface OperationalGraphNodeResponse {
  node?: OperationalGraphNodeStateRecord | null;
  current_links: OperationalGraphLinkStateRecord[];
  current_events: OperationalGraphEventStateRecord[];
  node_history: unknown[];
  link_history: unknown[];
  event_history: unknown[];
}

export interface OperationalHistoryRef {
  key: string;
  endpoint: string;
  count?: number | null;
  latest_at?: string | null;
}

export type OperationalHistoryRecord = Record<string, unknown>;

export interface OperationalRelationshipGroup {
  name: string;
  refs: OperationalObjectRef[];
}

export interface OperationalNavigation {
  self_ref?: OperationalObjectRef | null;
  graph?: OperationalGraphRef | null;
  history: OperationalHistoryRef[];
  relationships: OperationalRelationshipGroup[];
  neighborhood_endpoint?: string | null;
}

export interface OperationalNeighborhood {
  root: OperationalObjectRef;
  relationships: OperationalRelationshipGroup[];
  graph_ref?: OperationalGraphRef | null;
  history_refs: OperationalHistoryRef[];
}

export interface SuggestedToolCall {
  tool: string;
  args: Record<string, unknown>;
  reason: string;
}

export interface LiveMarketRegime {
  bias: string;
  confidence: number;
  breadth_up: number;
  breadth_down: number;
  average_return: number;
  directional_consensus?: number | null;
  pre_market_sentiment?: number | null;
}

export interface LiveStressSnapshot {
  composite_stress: number;
  sector_synchrony?: number | null;
  pressure_consensus?: number | null;
  momentum_consensus?: number | null;
  pressure_dispersion?: number | null;
  volume_anomaly?: number | null;
}

export interface MarketSessionRelationships {
  focus_symbols: OperationalObjectRef[];
}

export interface MarketSessionContract {
  id: { 0: string } | string;
  market: string;
  source_tick: number;
  observed_at: string;
  computed_at: string;
  market_regime: LiveMarketRegime;
  stress: LiveStressSnapshot;
  focus_symbols: string[];
  should_speak: boolean;
  priority: number;
  active_thread_count: number;
  wake_headline?: string | null;
  wake_summary: string[];
  wake_reasons: string[];
  suggested_tools: SuggestedToolCall[];
  market_summary?: string | null;
  navigation: OperationalNavigation;
  relationships: MarketSessionRelationships;
  focus_symbol_refs: OperationalObjectRef[];
}

export interface AgentSignalState {
  composite: number;
  mark_price?: number | null;
  capital_flow_direction: number;
  price_momentum: number;
  volume_profile: number;
  pre_post_market_anomaly: number;
  valuation: number;
  sector_coherence?: number | null;
  cross_stock_correlation?: number | null;
  cross_market_propagation?: number | null;
}

export interface AgentActionExpectancies {
  follow_expectancy?: number | null;
  fade_expectancy?: number | null;
  wait_expectancy?: number | null;
}

export interface AgentStructureState {
  symbol: string;
  sector?: string | null;
  setup_id?: string | null;
  title: string;
  action: string;
  status?: string | null;
  age_ticks?: number | null;
  status_streak?: number | null;
  confidence: number;
  confidence_change?: number | null;
  confidence_gap?: number | null;
  transition_reason?: string | null;
  contest_state?: string | null;
  current_leader?: string | null;
  leader_streak?: number | null;
  leader_transition_summary?: string | null;
  thesis_family?: string | null;
  expected_net_alpha?: number | null;
  alpha_horizon?: string | null;
  invalidation_rule?: string | null;
  follow_expectancy?: number | null;
  fade_expectancy?: number | null;
  wait_expectancy?: number | null;
}

export interface AgentDepthState {
  imbalance: number;
  imbalance_change: number;
  bid_best_ratio: number;
  bid_best_ratio_change: number;
  ask_best_ratio: number;
  ask_best_ratio_change: number;
  bid_top3_ratio: number;
  bid_top3_ratio_change: number;
  ask_top3_ratio: number;
  ask_top3_ratio_change: number;
  spread?: number | null;
  spread_change?: number | null;
  bid_total_volume: number;
  ask_total_volume: number;
  bid_total_volume_change: number;
  ask_total_volume_change: number;
  summary: string;
}

export interface AgentBrokerInstitution {
  institution_id: number;
  name: string;
  bid_positions: number[];
  ask_positions: number[];
  seat_count: number;
}

export interface AgentBrokerState {
  current: AgentBrokerInstitution[];
  entered: string[];
  exited: string[];
  switched_to_bid: string[];
  switched_to_ask: string[];
}

export interface AgentInvalidationState {
  status: string;
  invalidated: boolean;
  transition_reason?: string | null;
  leading_falsifier?: string | null;
  rules: string[];
}

export interface LiveEvent {
  kind: string;
  summary: string;
  magnitude: number;
  age_secs?: number | null;
  freshness?: number | null;
}

export interface LiveTemporalBar {
  horizon: string;
  symbol: string;
  bucket_started_at: string;
  open?: number | null;
  high?: number | null;
  low?: number | null;
  close?: number | null;
  composite_open: number;
  composite_high: number;
  composite_low: number;
  composite_close: number;
  composite_mean: number;
  capital_flow_sum: number;
  capital_flow_delta: number;
  volume_total: number;
  event_count: number;
  signal_persistence: number;
}

export interface LiveLineageMetric {
  horizon?: string | null;
  template: string;
  total: number;
  resolved: number;
  hits: number;
  hit_rate: number;
  mean_return: number;
}

export interface LiveSuccessPattern {
  family: string;
  signature: string;
  dominant_channels: string[];
  samples: number;
  mean_net_return: number;
  mean_strength: number;
  mean_coherence: number;
  mean_channel_diversity?: number | null;
  center_kind?: string | null;
  role?: string | null;
}

export interface AgentSymbolState {
  symbol: string;
  sector?: string | null;
  structure?: AgentStructureState | null;
  signal?: AgentSignalState | null;
  depth?: AgentDepthState | null;
  brokers?: AgentBrokerState | null;
  invalidation?: AgentInvalidationState | null;
  pressure?: unknown;
  active_position?: unknown;
  latest_events: LiveEvent[];
}

export interface SymbolStateSummary {
  symbol: string;
  sector?: string | null;
  structure_action?: string | null;
  structure_status?: string | null;
  signal_composite?: number | null;
  has_depth: boolean;
  has_brokers: boolean;
  invalidated: boolean;
  leading_falsifier?: string | null;
  latest_event_count: number;
}

export interface SymbolStateRelationships {
  cases: OperationalObjectRef[];
  recommendations: OperationalObjectRef[];
  macro_events: OperationalObjectRef[];
}

export interface SymbolStateContract {
  id: string;
  market: string;
  source_tick: number;
  observed_at: string;
  symbol: string;
  sector?: string | null;
  navigation: OperationalNavigation;
  relationships: SymbolStateRelationships;
  summary: SymbolStateSummary;
  graph_ref: OperationalGraphRef;
  state: AgentSymbolState;
}

export interface CaseRelationships {
  symbol: OperationalObjectRef;
  workflow?: OperationalObjectRef | null;
  recommendations: OperationalObjectRef[];
}

export interface CaseContract {
  id: string;
  setup_id: string;
  market: string;
  source_tick: number;
  observed_at: string;
  symbol: string;
  sector?: string | null;
  title: string;
  action: string;
  workflow_state: string;
  workflow_id?: string | null;
  execution_policy?: string | null;
  governance_reason_code?: string | null;
  governance_reason?: string | null;
  owner?: string | null;
  reviewer?: string | null;
  queue_pin?: string | null;
  confidence: number;
  confidence_gap?: number | null;
  thesis_family?: string | null;
  current_leader?: string | null;
  invalidation_rule?: string | null;
  expected_net_alpha?: number | null;
  alpha_horizon?: string | null;
  policy_primary?: string | null;
  policy_reason?: string | null;
  multi_horizon_gate_reason?: string | null;
  matched_success_pattern_signature?: string | null;
  recommendation_ids: string[];
  navigation: OperationalNavigation;
  relationships: CaseRelationships;
  symbol_ref: OperationalObjectRef;
  workflow_ref?: OperationalObjectRef | null;
  recommendation_refs: OperationalObjectRef[];
  graph_ref: OperationalGraphRef;
  history_refs: {
    workflow?: OperationalHistoryRef | null;
    reasoning?: OperationalHistoryRef | null;
    outcomes?: OperationalHistoryRef | null;
  };
}

export interface AgentLensComponent {
  lens_name: string;
  confidence: number;
  content: string;
  tags: string[];
}

export interface AgentDecisionAttribution {
  historical_expectancies?: Record<string, number>;
  live_expectancy_shift?: number | null;
  decisive_factors: string[];
}

export interface AgentRecommendationResolution {
  resolved_tick: number;
  ticks_elapsed: number;
  status: string;
  price_return: number;
  follow_realized_return: number;
  fade_realized_return: number;
  wait_regret: number;
  counterfactual_best_action: string;
  best_action_was_correct: boolean;
}

export interface AgentRecommendation {
  recommendation_id: string;
  tick: number;
  symbol: string;
  sector?: string | null;
  title?: string | null;
  action: string;
  action_label?: string | null;
  bias: string;
  severity: string;
  confidence: number;
  score: number;
  horizon_ticks: number;
  regime_bias: string;
  status?: string | null;
  why: string;
  why_components: AgentLensComponent[];
  primary_lens?: string | null;
  supporting_lenses: string[];
  review_lens?: string | null;
  watch_next: string[];
  do_not: string[];
  fragility: string[];
  transition?: string | null;
  thesis_family?: string | null;
  matched_success_pattern_signature?: string | null;
  state_transition?: string | null;
  best_action: string;
  decision_attribution: AgentDecisionAttribution;
  expected_net_alpha?: number | null;
  alpha_horizon: string;
  price_at_decision?: number | null;
  resolution?: AgentRecommendationResolution | null;
  invalidation_rule?: string | null;
  invalidation_components: AgentLensComponent[];
  execution_policy: string;
  governance: unknown;
  governance_reason_code: string;
  governance_reason: string;
  follow_expectancy?: number | null;
  fade_expectancy?: number | null;
  wait_expectancy?: number | null;
}

export interface RecommendationSummary {
  action: string;
  bias: string;
  severity: string;
  confidence: number;
  best_action: string;
  primary_lens?: string | null;
  matched_success_pattern_signature?: string | null;
  execution_policy: string;
  governance_reason_code: string;
}

export interface RecommendationRelationships {
  symbol: OperationalObjectRef;
  case?: OperationalObjectRef | null;
  workflow?: OperationalObjectRef | null;
}

export interface RecommendationContract {
  id: string;
  market: string;
  source_tick: number;
  observed_at: string;
  symbol: string;
  related_case_id?: string | null;
  related_setup_id?: string | null;
  related_workflow_id?: string | null;
  navigation: OperationalNavigation;
  relationships: RecommendationRelationships;
  summary: RecommendationSummary;
  symbol_ref: OperationalObjectRef;
  case_ref?: OperationalObjectRef | null;
  workflow_ref?: OperationalObjectRef | null;
  graph_ref: OperationalGraphRef;
  recommendation: AgentRecommendation;
  history_refs: {
    journal?: OperationalHistoryRef | null;
    workflow?: OperationalHistoryRef | null;
    outcomes?: OperationalHistoryRef | null;
  };
}

export interface AgentEventImpact {
  primary_scope: string;
  secondary_scopes: string[];
  affected_markets: string[];
  affected_sectors: string[];
  affected_symbols: string[];
  preferred_expression: string;
  requires_market_confirmation: boolean;
  decisive_factors: string[];
}

export interface AgentMacroEvent {
  event_id: string;
  tick: number;
  market: string;
  event_type: string;
  authority_level: string;
  headline: string;
  summary: string;
  confidence: number;
  confirmation_state: string;
  impact: AgentEventImpact;
  supporting_notice_ids: string[];
  promotion_reasons: string[];
}

export interface MacroEventSummary {
  headline: string;
  event_type: string;
  authority_level: string;
  confidence: number;
  confirmation_state: string;
  primary_scope: string;
  preferred_expression: string;
  affected_symbol_count: number;
  affected_sector_count: number;
}

export interface MacroEventRelationships {
  symbols: OperationalObjectRef[];
  cases: OperationalObjectRef[];
  recommendations: OperationalObjectRef[];
}

export interface MacroEventContract {
  id: string;
  market: string;
  source_tick: number;
  observed_at: string;
  navigation: OperationalNavigation;
  relationships: MacroEventRelationships;
  summary?: MacroEventSummary | null;
  graph_ref: OperationalGraphRef;
  event: AgentMacroEvent;
}

export interface AgentThread {
  symbol: string;
  sector?: string | null;
  status: string;
  first_tick: number;
  last_tick: number;
  idle_ticks: number;
  turns_observed: number;
  priority: number;
  title?: string | null;
  headline?: string | null;
  latest_summary?: string | null;
  last_transition?: string | null;
  current_leader?: string | null;
  invalidation_status?: string | null;
  reasons: string[];
}

export interface ThreadContract {
  id: string;
  market: string;
  source_tick: number;
  observed_at: string;
  navigation: OperationalNavigation;
  thread: AgentThread;
}

export interface WorkflowRelationships {
  cases: OperationalObjectRef[];
  recommendations: OperationalObjectRef[];
}

export interface WorkflowContract {
  id: string;
  market: string;
  source_tick: number;
  observed_at: string;
  stage: string;
  execution_policy?: string | null;
  governance_reason_code?: string | null;
  owner?: string | null;
  reviewer?: string | null;
  queue_pin?: string | null;
  synthetic: boolean;
  case_ids?: string[] | null;
  recommendation_ids?: string[] | null;
  navigation: OperationalNavigation;
  relationships: WorkflowRelationships;
  case_refs: OperationalObjectRef[];
  recommendation_refs: OperationalObjectRef[];
  history_refs: {
    events?: OperationalHistoryRef | null;
  };
}

export interface OperatorWorkItem {
  id: string;
  origin: string;
  grain: string;
  lane: string;
  status: string;
  priority: number;
  scope_kind: string;
  scope_id: string;
  title: string;
  summary: string;
  symbol?: string | null;
  sector?: string | null;
  best_action?: string | null;
  execution_policy?: string | null;
  governance_reason_code?: string | null;
  blocker?: string | null;
  owner?: string | null;
  reviewer?: string | null;
  queue_pin?: string | null;
  object_ref?: OperationalObjectRef | null;
  case_ref?: OperationalObjectRef | null;
  workflow_ref?: OperationalObjectRef | null;
  source_refs: OperationalObjectRef[];
  navigation: OperationalNavigation;
}

export interface RuntimeTaskRecord {
  id: string;
  label: string;
  kind: string;
  status: string;
  market?: string | null;
  owner?: string | null;
  detail?: string | null;
  metadata?: Record<string, unknown> | null;
  last_error?: string | null;
  created_at: string;
  updated_at: string;
  started_at?: string | null;
  completed_at?: string | null;
}

export interface BackwardEvidence {
  channel: string;
  statement: string;
  weight: number;
}

export interface BackwardLeadingCause {
  explanation: string;
  net_conviction: number;
  falsifier?: string | null;
  supporting_evidence: BackwardEvidence[];
}

export interface BackwardInvestigation {
  leaf_scope?: { Symbol?: string } | null;
  leaf_label?: string | null;
  leading_cause?: BackwardLeadingCause | null;
  contest_state?: string | null;
}

export interface AgentSectorFlow {
  sector: string;
  member_count: number;
  average_composite: number;
  average_capital_flow: number;
  leaders: string[];
  exceptions: string[];
  summary: string;
}

export interface AgentNotice {
  notice_id: string;
  tick: number;
  kind: string;
  symbol?: string | null;
  sector?: string | null;
  title: string;
  summary: string;
  significance: number;
}

export interface AgentTransition {
  from_tick: number;
  to_tick: number;
  symbol: string;
  sector?: string | null;
  setup_id?: string | null;
  title: string;
  from_state?: string | null;
  to_state: string;
  confidence: number;
  summary: string;
  transition_reason?: string | null;
}

export interface AgentMarketRecommendation {
  recommendation_id: string;
  tick: number;
  market: string;
  regime_bias: string;
  edge_layer: string;
  bias: string;
  best_action: string;
  preferred_expression: string;
  market_impulse_score: number;
  macro_regime_discontinuity: number;
  expected_net_alpha?: number | null;
  horizon_ticks: number;
  alpha_horizon: string;
  summary: string;
  why_not_single_name?: string | null;
  focus_sectors: string[];
  decisive_factors: string[];
  reference_symbols: string[];
  average_return_at_decision: number;
  execution_policy: string;
  governance: unknown;
  governance_reason_code: string;
  governance_reason: string;
  headline?: string | null;
  rationale?: string | null;
}

export interface AgentSectorRecommendation {
  recommendation_id: string;
  tick: number;
  market: string;
  sector: string;
  regime_bias: string;
  bias: string;
  best_action: string;
  summary: string;
  confidence?: number | null;
  rationale?: string | null;
}

export interface AgentMacroEventCandidate {
  candidate_id: string;
  tick: number;
  market: string;
  source_kind: string;
  source_name: string;
  event_type: string;
  authority_level: string;
  headline: string;
  summary: string;
  confidence: number;
  novelty_score: number;
  impact: AgentEventImpact;
}

export interface OperationalSidecars {
  sector_flows: AgentSectorFlow[];
  backward_investigations: BackwardInvestigation[];
  world_state?: unknown;
  macro_event_candidates: AgentMacroEventCandidate[];
  knowledge_links: unknown[];
  operator_work_items: OperatorWorkItem[];
}

export interface OperationalSnapshot {
  version: number;
  market: string;
  source_tick: number;
  observed_at: string;
  computed_at: string;
  market_session: MarketSessionContract;
  recent_turns: unknown[];
  notices: AgentNotice[];
  recent_transitions: AgentTransition[];
  symbols: SymbolStateContract[];
  cases: CaseContract[];
  market_recommendation?: AgentMarketRecommendation | null;
  sector_recommendations: AgentSectorRecommendation[];
  recommendations: RecommendationContract[];
  macro_events: MacroEventContract[];
  threads: ThreadContract[];
  workflows: WorkflowContract[];
  sidecars: OperationalSidecars;
  events: LiveEvent[];
  temporal_bars?: LiveTemporalBar[];
  lineage?: LiveLineageMetric[];
  success_patterns?: LiveSuccessPattern[];
}

export interface CaseWorkflowState {
  workflow_id: string;
  stage: string;
  execution_policy: string;
  governance: unknown;
  governance_reason_code: string;
  governance_reason: string;
  timestamp: string;
  actor?: string | null;
  owner?: string | null;
  reviewer?: string | null;
  queue_pin?: string | null;
  note?: string | null;
}
