use super::InwayConfig;

#[derive(Debug)]
pub enum Event {
    ConfigUpdated(InwayConfig),
}
