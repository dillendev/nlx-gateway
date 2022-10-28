use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub services: HashMap<String, Service>,
}

impl Hash for Config {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for service in self.services.iter() {
            service.hash(state);
        }
    }
}

#[derive(Debug, Clone, Default, Hash)]
pub struct Service {
    pub name: String,
    pub internal: bool,
    pub endpoint_url: String,
    pub documentation_url: String,
    pub tech_support_contact: String,
    pub public_support_contact: String,
    pub one_time_costs: i32,
    pub monthly_costs: i32,
    pub request_costs: i32,
}

/// Maps a service name to an HTTP endpoint
pub type ServiceInwayMap = HashMap<String, Arc<String>>;
