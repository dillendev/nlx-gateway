use std::{collections::HashMap, hash::Hash, sync::Arc};

use reqwest::Url;
use wyhash2::WyHash;

#[derive(Debug, Clone, Hash, PartialEq)]
pub enum State {
    Unknown = 0,
    Up = 1,
    Down = 2,
}

impl From<i32> for State {
    fn from(num: i32) -> Self {
        match num {
            1 => Self::Up,
            2 => Self::Down,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Hash)]
pub struct Inway {
    pub address: String,
    pub state: State,
}

#[derive(Debug, Clone, Hash)]
pub struct Costs {
    pub one_time: i32,
    pub monthly: i32,
    pub request: i32,
}

#[derive(Debug, Clone, Hash)]
pub struct Organization {
    pub name: String,
    pub serial_number: String,
}

#[derive(Debug, Clone, Hash)]
pub struct Service {
    pub name: String,
    pub documentation_url: String,
    pub api_specification_type: String,
    pub internal: bool,
    pub public_support_contact: String,
    pub inways: Vec<Inway>,
    pub costs: Costs,
    pub organization: Organization,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub services: HashMap<String, Vec<Service>, WyHash>,
}

impl Hash for Config {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (name, services) in self.services.iter() {
            name.hash(state);
            services.hash(state);
        }
    }
}

/// Maps an OIN to services to an Inway endpoint
pub type ServiceInways = HashMap<String, HashMap<String, Option<Arc<Url>>>>;
