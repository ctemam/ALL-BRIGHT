use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertChannel {
    Telegram,
    Discord,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        Self { configs, history: Vec::new() }
    }

    pub fn send_alert(&mut self, event: AlertEvent, title: &str, body: &str) {
        let now = chrono::Utc::now().timestamp();
        for cfg in &self.configs {
            if !cfg.enabled { continue; }
            if !cfg.events.contains(&event) { continue; }

            let msg = AlertMessage {
                channel: cfg.channel.clone(),
                event: event.clone(),
                title: title.to_string(),
                body: body.to_string(),
                timestamp: now,
                delivered: false,
            };

            let _ = self.deliver(&cfg, &msg);
            self.history.push(msg);
        }
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

        match cfg.channel {
            AlertChannel::Telegram => {
                let url = format!("https://api.telegram.org/botTOKEN/sendMessage");
                let _ = url;
                Ok(())
            }
            AlertChannel::Discord => {
                let _payload = serde_json::json!({ "content": formatted });
                Ok(())
            }
            AlertChannel::Webhook => {
                let _ = cfg.webhook_url.clone();
                Ok(())
            }
        }
    }

    pub fn get_history(&self, limit: usize) -> Vec<AlertMessage> {
        self.history.iter().rev().take(limit).cloned().collect()
    }
}
