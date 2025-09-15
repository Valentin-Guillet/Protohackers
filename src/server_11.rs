use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::net::TcpStream;
use tokio::{io::AsyncWriteExt, sync::Mutex};

use crate::{utils, TcpServer};

#[derive(PartialEq)]
enum PolicyType {
    Conserve,
    Cull,
}

impl PolicyType {
    pub fn to_byte(&self) -> u8 {
        match self {
            PolicyType::Conserve => 0xa0,
            PolicyType::Cull => 0x90,
        }
    }
}

struct Policy {
    id: Option<u32>,
    species: String,
    policy_type: PolicyType,
}

enum PolicyAction {
    Delete { id: u32, species: String },
    Add { policy: Policy },
}

type SiteId = u32;

type ServerResult = Result<ServerMessage, &'static str>;

struct PopulationTarget {
    species: String,
    min: u32,
    max: u32,
}

struct PopulationObs {
    species: String,
    count: u32,
}

enum ServerMessage {
    Hello {
        protocol: String,
        version: u32,
    },
    Error {
        msg: String,
    },
    Ok,
    DialAuthority {
        site: u32,
    },
    TargetPopulations {
        site: u32,
        targets: Vec<PopulationTarget>,
    },
    CreatePolicy {
        species: String,
        action: u8,
    },
    DeletePolicy {
        policy: u32,
    },
    PolicyResult {
        policy: u32,
    },
    SiteVisit {
        site: u32,
        observations: Vec<PopulationObs>,
    },
}

impl ServerMessage {
    fn parse_u8(data: &[u8], index: &mut usize) -> Result<u8, &'static str> {
        if *index + 1 > data.len() {
            return Err("Error parsing u8: not enough bytes to read");
        }
        let ans = data[*index];
        *index += 1;
        Ok(ans)
    }

    fn parse_u32(data: &[u8], index: &mut usize) -> Result<u32, &'static str> {
        if *index + 4 > data.len() {
            return Err("Error parsing u32: not enough bytes to read");
        }
        let ans = u32::from_be_bytes(data[*index..*index + 4].try_into().unwrap());
        *index += 4;
        Ok(ans)
    }

    fn parse_str(data: &[u8], index: &mut usize) -> Result<String, &'static str> {
        let str_len = Self::parse_u32(data, index)? as usize;
        if *index + str_len > data.len() {
            return Err("Error parsing str: not enough bytes to read");
        }
        let ans = String::from_utf8_lossy(&data[*index..*index + str_len]).to_string();
        *index += str_len;
        Ok(ans)
    }

    fn parse_target_populations(
        data: &[u8],
        index: &mut usize,
    ) -> Result<Vec<PopulationTarget>, &'static str> {
        let pop_len = Self::parse_u32(data, index)?;
        let mut targets = Vec::new();
        for _ in 0..pop_len {
            let species = Self::parse_str(data, index)?;
            let min = Self::parse_u32(data, index)?;
            let max = Self::parse_u32(data, index)?;
            targets.push(PopulationTarget { species, min, max });
        }
        Ok(targets)
    }

    fn parse_population_obs(
        data: &[u8],
        index: &mut usize,
    ) -> Result<Vec<PopulationObs>, &'static str> {
        let pop_len = Self::parse_u32(data, index)?;
        let mut observations = Vec::new();
        for _ in 0..pop_len {
            let species = Self::parse_str(data, index)?;
            let count = Self::parse_u32(data, index)?;

            if !observations.iter().all(
                |PopulationObs {
                     species: sp,
                     count: c,
                 }| species != *sp || count == *c,
            ) {
                return Err("Error in population observation: conflicting counts");
            }

            observations.push(PopulationObs { species, count });
        }
        Ok(observations)
    }

    fn parse_msg_hello(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let protocol = Self::parse_str(data, &mut index)?;
        let version = Self::parse_u32(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing Hello: found additional data");
        }
        Ok(ServerMessage::Hello { protocol, version })
    }

    fn parse_msg_error(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let msg = Self::parse_str(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing Error: found additional data");
        }
        Ok(ServerMessage::Error { msg })
    }

    fn parse_msg_ok(data: &[u8]) -> ServerResult {
        if !data.is_empty() {
            return Err("Error parsing Ok: found additional data");
        }
        Ok(ServerMessage::Ok)
    }

    fn parse_msg_dial_authority(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let site = Self::parse_u32(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing DialAuthority: found additional data");
        }
        Ok(ServerMessage::DialAuthority { site })
    }

    fn parse_msg_target_population(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let site = Self::parse_u32(data, &mut index)?;
        let targets = Self::parse_target_populations(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing TargetPopulations: found additional data");
        }
        Ok(ServerMessage::TargetPopulations { site, targets })
    }

    fn parse_msg_create_policy(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let species = Self::parse_str(data, &mut index)?;
        let action = Self::parse_u8(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing CreatePolicy: found additional data");
        }
        Ok(ServerMessage::CreatePolicy { species, action })
    }

    fn parse_msg_delete_policy(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let policy = Self::parse_u32(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing DeletePolicy: found additional data");
        }
        Ok(ServerMessage::DeletePolicy { policy })
    }

    fn parse_msg_policy_result(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let policy = Self::parse_u32(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing PolicyResult: found additional data");
        }
        Ok(ServerMessage::PolicyResult { policy })
    }

    fn parse_msg_site_visit(data: &[u8]) -> ServerResult {
        let mut index = 0;
        let site = Self::parse_u32(data, &mut index)?;
        let observations = Self::parse_population_obs(data, &mut index)?;
        if index != data.len() {
            return Err("Error parsing PopulationObs: found additional data");
        }
        Ok(ServerMessage::SiteVisit { site, observations })
    }

    pub fn parse(msg_type: u8, data: &[u8]) -> ServerResult {
        match msg_type {
            0x50 => Self::parse_msg_hello(data),
            0x51 => Self::parse_msg_error(data),
            0x52 => Self::parse_msg_ok(data),
            0x53 => Self::parse_msg_dial_authority(data),
            0x54 => Self::parse_msg_target_population(data),
            0x55 => Self::parse_msg_create_policy(data),
            0x56 => Self::parse_msg_delete_policy(data),
            0x57 => Self::parse_msg_policy_result(data),
            0x58 => Self::parse_msg_site_visit(data),
            _ => Err("Invalid message type"),
        }
    }

    fn u32_to_bytes(data: u32) -> Vec<u8> {
        data.to_be_bytes().into()
    }

    fn str_to_bytes(data: &str) -> Vec<u8> {
        let mut bytes = Vec::from(&(data.len() as u32).to_be_bytes());
        bytes.extend(data.as_bytes());
        bytes
    }

    fn target_population_to_bytes(data: &[PopulationTarget]) -> Vec<u8> {
        let mut bytes = Self::u32_to_bytes(data.len() as u32);
        bytes.extend(
            data.iter()
                .flat_map(|PopulationTarget { species, min, max }| {
                    let mut pop_bytes = Self::str_to_bytes(species);
                    pop_bytes.extend(Self::u32_to_bytes(*min));
                    pop_bytes.extend(Self::u32_to_bytes(*max));
                    pop_bytes
                }),
        );
        bytes
    }

    fn population_obs_to_bytes(data: &[PopulationObs]) -> Vec<u8> {
        let mut bytes = Self::u32_to_bytes(data.len() as u32);
        bytes.extend(data.iter().flat_map(|PopulationObs { species, count }| {
            let mut pop_bytes = Self::str_to_bytes(species);
            pop_bytes.extend(Self::u32_to_bytes(*count));
            pop_bytes
        }));
        bytes
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let (msg_type, data_bytes) = match self {
            ServerMessage::Hello { protocol, version } => {
                let mut bytes = Self::str_to_bytes(protocol);
                bytes.extend(Self::u32_to_bytes(*version));
                (0x50, bytes)
            }
            ServerMessage::Error { msg } => (0x51, Self::str_to_bytes(msg)),
            ServerMessage::Ok => (0x52, Vec::new()),
            ServerMessage::DialAuthority { site } => (0x53, Self::u32_to_bytes(*site)),
            ServerMessage::TargetPopulations { site, targets } => {
                let mut bytes = Self::u32_to_bytes(*site);
                bytes.extend(Self::target_population_to_bytes(targets));
                (0x54, bytes)
            }
            ServerMessage::CreatePolicy { species, action } => {
                let mut bytes = Self::str_to_bytes(species);
                bytes.push(*action);
                (0x55, bytes)
            }
            ServerMessage::DeletePolicy { policy } => (0x56, Self::u32_to_bytes(*policy)),
            ServerMessage::PolicyResult { policy } => (0x57, Self::u32_to_bytes(*policy)),
            ServerMessage::SiteVisit { site, observations } => {
                let mut bytes = Self::u32_to_bytes(*site);
                bytes.extend(Self::population_obs_to_bytes(observations));
                (0x58, bytes)
            }
        };

        let mut bytes = Vec::from([msg_type]);
        bytes.extend(Self::u32_to_bytes(data_bytes.len() as u32 + 6));
        bytes.extend(data_bytes);

        let checksum = bytes
            .iter()
            .fold(0u8, |acc, &v| acc.wrapping_add(v))
            .wrapping_neg();
        bytes.push(checksum);
        bytes
    }
}

struct SiteState {
    targets: HashMap<String, PopulationTarget>,
    policies: HashMap<String, Policy>,
}

impl SiteState {
    fn new() -> Self {
        Self {
            targets: HashMap::new(),
            policies: HashMap::new(),
        }
    }

    fn get_action(&self, species: &str, count: u32) -> Vec<PolicyAction> {
        let mut actions = Vec::new();
        let Some(target) = self.targets.get(species) else {
            return actions;
        };

        let last_policy = self.policies.get(species);
        if (count < target.min
            && last_policy.is_some_and(|policy| policy.policy_type == PolicyType::Conserve))
            || (count > target.max
                && last_policy.is_some_and(|policy| policy.policy_type == PolicyType::Cull))
            || (target.min <= count && count <= target.max && last_policy.is_none())
        {
            return actions;
        }

        if let Some(policy) = last_policy {
            actions.push(PolicyAction::Delete {
                id: policy.id.unwrap(),
                species: species.to_owned(),
            });
        }

        if count < target.min || count > target.max {
            actions.push(PolicyAction::Add {
                policy: Policy {
                    id: None,
                    species: species.to_owned(),
                    policy_type: if count < target.min {
                        PolicyType::Conserve
                    } else {
                        PolicyType::Cull
                    },
                },
            });
        }

        actions
    }

    pub fn get_actions(&mut self, observations: &[PopulationObs]) -> Vec<PolicyAction> {
        let mut all_species_obs = self
            .targets
            .keys()
            .map(|species| (species, 0u32))
            .collect::<HashMap<_, _>>();
        for PopulationObs { species, count } in observations {
            all_species_obs.entry(species).and_modify(|c| *c = *count);
        }
        all_species_obs
            .iter()
            .flat_map(|(&species, &count)| self.get_action(species, count))
            .collect()
    }
}

pub struct Server {
    auth_connections: Arc<Mutex<HashMap<SiteId, Arc<Mutex<TcpStream>>>>>,
    site_states: Arc<Mutex<HashMap<SiteId, Arc<Mutex<SiteState>>>>>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            auth_connections: Arc::new(Mutex::new(HashMap::new())),
            site_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn parse_message(&self, stream: &mut TcpStream, buffer: &mut Vec<u8>) -> ServerResult {
        let Some(msg_header) = utils::read_for(stream, buffer, 5).await else {
            return Err("Couldn't read message header");
        };
        let msg_type = msg_header[0];
        let msg_len = u32::from_be_bytes(msg_header[1..5].try_into().unwrap());

        let Some(mut data) = utils::read_for(stream, buffer, msg_len as usize - 5).await else {
            return Err("Invalid message length");
        };

        let mut checksum = msg_header.iter().fold(0u8, |acc, &v| acc.wrapping_add(v));
        checksum = checksum.wrapping_add(data.iter().fold(0u8, |acc, &v| acc.wrapping_add(v)));
        if checksum != 0 {
            return Err("Invalid checksum");
        }

        let Some(_) = data.pop() else {
            return Err("Empty message received");
        };

        ServerMessage::parse(msg_type, &data)
    }

    async fn get_connection(&self, site: u32) -> Arc<Mutex<TcpStream>> {
        let mut connections = self.auth_connections.lock().await;
        if let Entry::Vacant(entry) = connections.entry(site) {
            let new_connection = TcpStream::connect("pestcontrol.protohackers.com:20547")
                .await
                .expect("Could not connect to authority server");
            entry.insert(Arc::new(Mutex::new(new_connection)));
        }
        Arc::clone(connections.get(&site).unwrap())
    }

    async fn get_targets(
        &self,
        site: u32,
    ) -> Result<HashMap<String, PopulationTarget>, &'static str> {
        let connection = self.get_connection(site).await;
        let mut connection = connection.lock().await;
        let mut buffer = Vec::new();
        let msg = ServerMessage::Hello {
            protocol: "pestcontrol".into(),
            version: 1,
        };
        let _ = connection.write_all(&msg.to_bytes()).await;
        let response = self.parse_message(&mut connection, &mut buffer).await?;
        match response {
            ServerMessage::Hello {
                protocol,
                version: 1,
            } if protocol == "pestcontrol" => (),
            ServerMessage::Hello { .. } => {
                return Err("Invalid Hello message from authority server")
            }
            _ => return Err("No Hello message from authority server"),
        };

        let msg = ServerMessage::DialAuthority { site };
        let _ = connection.write_all(&msg.to_bytes()).await;
        let response = self.parse_message(&mut connection, &mut buffer).await?;
        let ServerMessage::TargetPopulations { targets, .. } = response else {
            return Err("Invalid TargetPopulations message from authority server");
        };
        Ok(targets
            .into_iter()
            .map(|t| (t.species.to_owned(), t))
            .collect())
    }

    async fn get_site_state(&self, site: u32) -> Result<Arc<Mutex<SiteState>>, &'static str> {
        let mut site_states = self.site_states.lock().await;
        if site_states.contains_key(&site) {
            return Ok(Arc::clone(site_states.get(&site).unwrap()));
        }

        let new_site_state = Arc::new(Mutex::new(SiteState::new()));
        let site_state_am = Arc::clone(&new_site_state);
        let mut site_state = site_state_am.lock().await;
        site_states.insert(site, new_site_state);
        drop(site_states);

        site_state.targets = self.get_targets(site).await?;
        drop(site_state);
        Ok(site_state_am)
    }

    async fn add_policy(&self, site: u32, policy: &mut Policy) -> Result<(), &'static str> {
        let connection = self.get_connection(site).await;
        let mut connection = connection.lock().await;

        let mut buffer = Vec::new();
        let msg = ServerMessage::CreatePolicy {
            species: policy.species.to_owned(),
            action: policy.policy_type.to_byte(),
        };
        let _ = connection.write_all(&msg.to_bytes()).await;
        let response = self.parse_message(&mut connection, &mut buffer).await?;
        policy.id = match response {
            ServerMessage::PolicyResult { policy } => Some(policy),
            _ => return Err("Error when creating policy"),
        };
        Ok(())
    }

    async fn delete_policy(&self, site: u32, policy_id: u32) -> Result<(), &'static str> {
        let connection = self.get_connection(site).await;
        let mut connection = connection.lock().await;

        let mut buffer = Vec::new();
        let msg = ServerMessage::DeletePolicy { policy: policy_id };
        let _ = connection.write_all(&msg.to_bytes()).await;
        let response = self.parse_message(&mut connection, &mut buffer).await?;
        match response {
            ServerMessage::Ok => (),
            _ => return Err("Error when deleting policy"),
        };
        Ok(())
    }

    async fn process_observation(
        &self,
        site: u32,
        observations: Vec<PopulationObs>,
    ) -> Result<(), &'static str> {
        let site_state = self.get_site_state(site).await?;
        let mut site_state = site_state.lock().await;
        for action in site_state.get_actions(&observations) {
            match action {
                PolicyAction::Delete { id, species } => {
                    self.delete_policy(site, id).await?;
                    site_state.policies.remove_entry(&species);
                }
                PolicyAction::Add { mut policy } => {
                    self.add_policy(site, &mut policy).await?;
                    site_state
                        .policies
                        .insert(policy.species.to_owned(), policy);
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = Vec::new();

        let first_message = self.parse_message(&mut stream, &mut buffer).await;
        let mut buffer = Vec::new();
        let msg = ServerMessage::Hello {
            protocol: "pestcontrol".into(),
            version: 1,
        };
        let _ = stream.write_all(&msg.to_bytes()).await;

        match first_message {
            Ok(ServerMessage::Hello {
                protocol,
                version: 1,
            }) if protocol == "pestcontrol" => (),
            Ok(ServerMessage::Hello { protocol, version }) => {
                let response = ServerMessage::Error {
                    msg: format!("Invalid Hello message (protocol: {protocol}, version {version})",),
                };
                let _ = stream.write_all(&response.to_bytes()).await;
                return;
            }
            Ok(_) => {
                let response = ServerMessage::Error {
                    msg: String::from("Connection must start with a Hello message"),
                };
                let _ = stream.write_all(&response.to_bytes()).await;
                return;
            }
            Err(msg) => {
                let response = ServerMessage::Error { msg: msg.into() };
                let _ = stream.write_all(&response.to_bytes()).await;
                return;
            }
        };

        loop {
            let (site, populations) = match self.parse_message(&mut stream, &mut buffer).await {
                Ok(ServerMessage::SiteVisit { site, observations }) => (site, observations),
                Ok(_) => {
                    let response = ServerMessage::Error {
                        msg: "Invalid message type from site-visiting client".into(),
                    };
                    let _ = stream.write_all(&response.to_bytes()).await;
                    break;
                }
                Err(msg) => {
                    let response = ServerMessage::Error { msg: msg.into() };
                    let _ = stream.write_all(&response.to_bytes()).await;
                    break;
                }
            };

            if let Err(msg) = self.process_observation(site, populations).await {
                let response = ServerMessage::Error { msg: msg.into() };
                let _ = stream.write_all(&response.to_bytes()).await;
                break;
            }
        }
    }
}
