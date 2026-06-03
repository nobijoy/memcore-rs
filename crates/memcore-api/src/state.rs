use chrono::{DateTime, Utc};
use memcore_config::Settings;

#[derive(Clone, Debug)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            started_at: Utc::now(),
        }
    }
}
