use super::InwayConfig;

#[derive(Debug, Clone)]
pub enum Event {
    ConfigUpdated(InwayConfig),
}
