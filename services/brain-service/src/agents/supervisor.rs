//! Supervisor Agent
//!
//! Routes to the correct domain agent based on detected intent.
//! All agents are subordinate to the supervisor.
#![allow(dead_code)] // Reserved for the multi-agent routing milestone.

use crate::intents::Intent;

pub struct SupervisorAgent;

impl SupervisorAgent {
    pub fn select_agent(intent: &Intent) -> String {
        match intent {
            Intent::ReportDisease | Intent::AskFertilizerRecommendation => {
                "agronomist_agent".into()
            }

            Intent::CheckStock | Intent::CheckHarvestStatus | Intent::ReportActivity => {
                "farm_agent".into()
            }

            Intent::RecordExpense
            | Intent::RecordRevenue
            | Intent::RequestLoan
            | Intent::CheckCreditScore => "finance_agent".into(),

            Intent::BuyProduct | Intent::CheckPrice => "marketplace_agent".into(),

            Intent::AskWeather => "farm_agent".into(),

            Intent::Unknown => "farm_agent".into(), // safe default
        }
    }
}
