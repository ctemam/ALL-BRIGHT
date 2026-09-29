use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertChannel {
    Telegram,
    Discord,
    Webhook,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertEvent {
    TradeExecuted,
    OpportunityFound,
    ProfitTaken,
    LossTriggered,
    ErrorOccurred,
    MevDetected,
    BotStarted,
    BotStopped,
    DailySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub channel: AlertChannel,
    pub webhook_url: String,
    pub events: Vec<AlertEvent>,
    pub min_profit_usd: f64,
    pub enabled: bool,
    pub notify_on_error: bool,
    pub daily_summary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertMessage {
    pub channel: AlertChannel,
    pub event: AlertEvent,
    pub title: String,
    pub body: String,
    pub timestamp: i64,
    pub delivered: bool,
}

pub struct AlertManager {
    pub configs: Vec<AlertConfig>,
    pub history: Vec<AlertMessage>,
}

impl AlertManager {
    pub fn new(configs: Vec<AlertConfig>) -> Self {
        Self {
            configs,
            history: Vec::new(),
        }
    }

    /// Sends an alert to every enabled channel subscribed to `event`.
    ///
    /// Returns one message per channel that failed to deliver. Delivery is not implemented
    /// yet, so this always reports failures and records history with `delivered: false`;
    /// previously every channel returned `Ok(())` while nothing was ever sent (D-04).
    pub fn send_alert(&mut self, event: AlertEvent, title: &str, body: &str) -> Vec<String> {
        let now = chrono::Utc::now().timestamp();
        let mut failures = Vec::new();
        for cfg in &self.configs {
            if !cfg.enabled {
                continue;
            }
            if !cfg.events.contains(&event) {
                continue;
            }

            let msg = AlertMessage {
                channel: cfg.channel.clone(),
                event: event.clone(),
                title: title.to_string(),
                body: body.to_string(),
                timestamp: now,
                delivered: false,
            };

            let delivered = match self.deliver(cfg, &msg) {
                Ok(()) => true,
                Err(e) => {
                    failures.push(format!("{:?}: {}", cfg.channel, e));
                    false
                }
            };

            self.history.push(AlertMessage { delivered, ..msg });
        }
        failures
    }

    fn deliver(&self, cfg: &AlertConfig, msg: &AlertMessage) -> Result<(), String> {
        let emoji = match msg.event {
            AlertEvent::TradeExecuted => "✅",
            AlertEvent::OpportunityFound => "💰",
            AlertEvent::ProfitTaken => "📈",
            AlertEvent::LossTriggered => "📉",
            AlertEvent::ErrorOccurred => "❌",
            AlertEvent::MevDetected => "🚨",
            AlertEvent::BotStarted => "🤖",
            AlertEvent::BotStopped => "🛑",
            AlertEvent::DailySummary => "📊",
        };

        let formatted = format!("{} *{}*\n{}", emoji, msg.title, msg.body);

        // NOTE: delivery is intentionally not implemented. This used to build a Telegram URL
        // containing the literal string "botTOKEN", throw away the Discord payload and
        // return Ok(()) for every channel, so the UI reported success while nothing was
        // ever sent (D-04). Returning an error keeps the gap visible and marks the
        // history entry as undelivered.
        match cfg.channel {
            AlertChannel::Telegram => Err(
                "Telegram delivery is not implemented (no bot token or HTTP client wired up)"
                    .to_string(),
            ),
            AlertChannel::Discord => Err(format!(
                "Discord delivery is not implemented (payload of {} chars prepared but not sent)",
                formatted.len()
            )),
            AlertChannel::Webhook => {
                let target = if cfg.webhook_url.is_empty() {
                    "<unset>".to_string()
                } else {
                    cfg.webhook_url.clone()
                };
                Err(format!(
                    "Webhook delivery is not implemented (target: {})",
                    target
                ))
            }
        }
    }

    pub fn get_history(&self, limit: usize) -> Vec<AlertMessage> {
        self.history.iter().rev().take(limit).cloned().collect()
    }
}
