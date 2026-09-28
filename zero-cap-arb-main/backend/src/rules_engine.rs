use crate::types::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleOperator {
    GreaterThan,
    LessThan,
    Equals,
    Between,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleField {
    SpreadPct,
    NetProfitUsd,
    ConfidenceScore,
    LiquidityUsd,
    GasCostUsd,
    ChainId,
    ArbitrageType,
    TokenSymbol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleAction {
    Execute,
    Skip,
    LogOnly,
    NotifyMe,
    AskApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub field: RuleField,
    pub operator: RuleOperator,
    pub value: String,
    pub action: RuleAction,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEvaluationResult {
    pub rule_id: String,
    pub rule_name: String,
    pub matched: bool,
    pub action: RuleAction,
    pub reason: String,
}

pub struct RulesEngine {
    pub rules: Vec<ExecutionRule>,
}

impl RulesEngine {
    pub fn new(rules: Vec<ExecutionRule>) -> Self {
        Self { rules }
    }

    pub fn evaluate(&self, opportunity: &OpportunityDetail) -> Vec<RuleEvaluationResult> {
        let mut results = Vec::new();
        for rule in &self.rules {
            if !rule.enabled { continue; }
            let field_value = self.get_field_value(opportunity, &rule.field);
            let matched = self.evaluate_operator(&field_value, &rule.operator, &rule.value);
            results.push(RuleEvaluationResult {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                matched,
                action: rule.action.clone(),
                reason: if matched {
                    format!("Rule '{}' matched: {} {} {}", rule.name, format!("{:?}", rule.field),
                        format!("{:?}", rule.operator), rule.value)
                } else {
                    format!("Rule '{}' not matched", rule.name)
                },
            });
        }
        results
    }

    pub fn should_execute(&self, opportunity: &OpportunityDetail) -> (bool, Vec<RuleEvaluationResult>) {
        let results = self.evaluate(opportunity);
        let execute = results.iter().all(|r| {
            if r.matched {
                match r.action {
                    RuleAction::Execute | RuleAction::LogOnly => true,
                    RuleAction::Skip => false,
                    RuleAction::NotifyMe => true,
                    RuleAction::AskApproval => false,
                }
            } else { true }
        });
        (execute, results)
    }

    fn get_field_value(&self, opp: &OpportunityDetail, field: &RuleField) -> String {
        match field {
            RuleField::SpreadPct => opp.spread_pct.to_string(),
            RuleField::NetProfitUsd => opp.profit_breakdown.net_profit_usd.to_string(),
            RuleField::ConfidenceScore => opp.confidence_score.to_string(),
            RuleField::LiquidityUsd => opp.liquidity_usd.to_string(),
            RuleField::GasCostUsd => opp.profit_breakdown.costs.gas_estimated_usd.to_string(),
            RuleField::ChainId => opp.chain_id.to_string(),
            RuleField::ArbitrageType => format!("{:?}", opp.arbitrage_type),
            RuleField::TokenSymbol => opp.token.clone(),
        }
    }

    fn evaluate_operator(&self, field_value: &str, operator: &RuleOperator, rule_value: &str) -> bool {
        match operator {
            RuleOperator::GreaterThan => {
                let fv: f64 = field_value.parse().unwrap_or(0.0);
                let rv: f64 = rule_value.parse().unwrap_or(0.0);
                fv > rv
            }
            RuleOperator::LessThan => {
                let fv: f64 = field_value.parse().unwrap_or(0.0);
                let rv: f64 = rule_value.parse().unwrap_or(0.0);
                fv < rv
            }
            RuleOperator::Equals => field_value == rule_value,
            RuleOperator::Between => {
                if let Some((lo, hi)) = rule_value.split_once(',') {
                    let fv: f64 = field_value.parse().unwrap_or(0.0);
                    let l: f64 = lo.trim().parse().unwrap_or(0.0);
                    let h: f64 = hi.trim().parse().unwrap_or(0.0);
                    fv >= l && fv <= h
                } else { false }
            }
            RuleOperator::Contains => field_value.to_lowercase().contains(&rule_value.to_lowercase()),
        }
    }
}
