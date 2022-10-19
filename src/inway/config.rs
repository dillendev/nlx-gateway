use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Default)]
pub struct InwayConfig {
    pub services: HashMap<String, Service>,
}

impl Hash for InwayConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for service in self.services.iter() {
            service.hash(state);
        }
    }
}

#[derive(Debug, Clone, Default, Hash)]
pub struct Service {
    pub name: String,
}
