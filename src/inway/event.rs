use super::InwayConfig;

#[derive(Debug)]
pub enum Event {
    InwayRegistered,
    ConfigUpdated(InwayConfig),
}
