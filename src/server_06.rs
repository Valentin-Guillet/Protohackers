use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time;

use crate::{utils, TcpServer};

type ServerResult = Result<Vec<ServerMessage>, &'static str>;

type Id = u16;

struct Plate {
    id: Id,
    plate: String,
    timestamp: u32,
}

struct Heartbeat {
    id: Id,
    interval: u32,
}

struct Camera {
    id: Id,
    road: u16,
    mile: u16,
    limit: u16,
}

#[derive(Clone)]
struct Dispatcher {
    id: Id,
    roads: Vec<u16>,
}

#[derive(Clone)]
struct Observation {
    plate: String,
    timestamp: u32,
    road: u16,
    mile: u16,
}

#[derive(Clone)]
enum ServerMessage {
    Ticket {
        recipient: Option<Id>,
        plate: String,
        road: u16,
        mile1: u16,
        timestamp1: u32,
        mile2: u16,
        timestamp2: u32,
        speed: u16,
    },
    WantHeartbeat {
        interval: u32,
    },
}

struct ServerState {
    cameras: Vec<Camera>,
    dispatchers: Vec<Dispatcher>,
    observations: Vec<Observation>,
    have_heartbeats: Vec<Id>,
    ticket_queue: Vec<ServerMessage>,
    ticket_sent: HashMap<String, Vec<u32>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            cameras: Vec::new(),
            dispatchers: Vec::new(),
            observations: Vec::new(),
            have_heartbeats: Vec::new(),
            ticket_queue: Vec::new(),
            ticket_sent: HashMap::new(),
        }
    }

    fn has_client(&self, client_id: Id) -> bool {
        return self
            .cameras
            .iter()
            .find(|camera| camera.id == client_id)
            .is_some()
            || self
                .dispatchers
                .iter()
                .find(|dispatcher| dispatcher.id == client_id)
                .is_some();
    }

    fn compute_speed(obs1: &Observation, obs2: &Observation) -> u16 {
        let dist = obs1.mile.abs_diff(obs2.mile) as f32;
        let time = obs1.timestamp.abs_diff(obs2.timestamp) as f32;
        (3600f32 * dist / time).round() as u16
    }

    fn get_dispatcher(&self, road: u16) -> Option<Id> {
        self.dispatchers
            .iter()
            .filter(|&dispatcher| dispatcher.roads.contains(&road))
            .map(|dispatcher| dispatcher.id)
            .next()
    }

    fn generate_tickets(
        &mut self,
        obs1: &Observation,
        obs2: &Observation,
        speed: u16,
    ) -> ServerResult {
        let start_ts = obs1.timestamp.min(obs2.timestamp);
        let end_ts = obs1.timestamp.max(obs2.timestamp);
        let start_day = start_ts / 86400;
        let end_day = end_ts / 86400;

        if self
            .ticket_sent
            .get(&obs1.plate)
            .is_some_and(|days| days.iter().any(|day| (start_day..=end_day).contains(&day)))
        {
            return Ok(Vec::new());
        }
        self.ticket_sent
            .entry(obs1.plate.clone())
            .and_modify(|days| days.extend(start_day..=end_day))
            .or_insert((start_day..=end_day).collect());

        let dispatcher = self.get_dispatcher(obs1.road);

        let (mile1, mile2) = if obs1.timestamp < obs2.timestamp {
            (obs1.mile, obs2.mile)
        } else {
            (obs2.mile, obs1.mile)
        };
        let ticket = ServerMessage::Ticket {
            recipient: dispatcher,
            plate: obs1.plate.clone(),
            road: obs1.road,
            mile1,
            timestamp1: start_ts,
            mile2,
            timestamp2: end_ts,
            speed: 100 * speed,
        };

        if dispatcher.is_some() {
            Ok(vec![ticket])
        } else {
            self.ticket_queue.push(ticket);
            Ok(Vec::new())
        }
    }

    fn read_plate(&mut self, plate: Plate) -> ServerResult {
        let camera = match self.cameras.iter().find(|&camera| camera.id == plate.id) {
            Some(camera) => camera,
            None => return Err("non-camera can't read plate"),
        };

        let observation = Observation {
            plate: plate.plate,
            timestamp: plate.timestamp,
            road: camera.road,
            mile: camera.mile,
        };

        let relevant_obs = self
            .observations
            .iter()
            .cloned()
            .filter(|obs| obs.plate == observation.plate && obs.road == observation.road)
            .collect::<Vec<_>>();

        self.observations.push(observation.clone());
        let mut tickets = Vec::new();
        let speed_limit = camera.limit;
        for obs in relevant_obs {
            let avg_speed = Self::compute_speed(&observation, &obs);
            if avg_speed > speed_limit {
                tickets.extend_from_slice(&self.generate_tickets(&observation, &obs, avg_speed)?);
            }
        }
        Ok(tickets)
    }

    fn mark_heartbeat(&mut self, heartbeat: Heartbeat) -> ServerResult {
        if self.have_heartbeats.contains(&heartbeat.id) {
            Err("client already asked for heartbeat")
        } else {
            self.have_heartbeats.push(heartbeat.id);
            Ok(vec![ServerMessage::WantHeartbeat {
                interval: heartbeat.interval,
            }])
        }
    }

    fn add_camera(&mut self, camera: Camera) -> ServerResult {
        if self.has_client(camera.id) {
            Err("client was already a camera or a dispatcher")
        } else {
            self.cameras.push(camera);
            Ok(Vec::new())
        }
    }

    fn add_dispatcher(&mut self, dispatcher: Dispatcher) -> ServerResult {
        if self.has_client(dispatcher.id) {
            return Err("client was already a camera or a dispatcher");
        }
        self.dispatchers.push(dispatcher.clone());

        let (to_send, new_ticket_queue) =
            self.ticket_queue
                .iter()
                .cloned()
                .partition(|ticket| match ticket {
                    ServerMessage::Ticket { road, .. } => dispatcher.roads.contains(&road),
                    _ => unreachable!(),
                });
        self.ticket_queue = new_ticket_queue;

        Ok(to_send
            .into_iter()
            .map(|ticket| match ticket {
                ServerMessage::Ticket {
                    recipient: _,
                    plate,
                    road,
                    mile1,
                    timestamp1,
                    mile2,
                    timestamp2,
                    speed,
                } => ServerMessage::Ticket {
                    recipient: Some(dispatcher.id),
                    plate,
                    road,
                    mile1,
                    timestamp1,
                    mile2,
                    timestamp2,
                    speed,
                },
                _ => unreachable!(),
            })
            .collect())
    }

    fn remove_client(&mut self, client_id: Id) {
        if let Some(pos) = self.cameras.iter().position(|c| c.id == client_id) {
            self.cameras.swap_remove(pos);
        }

        if let Some(pos) = self.dispatchers.iter().position(|c| c.id == client_id) {
            self.dispatchers.swap_remove(pos);
        }

        if let Some(pos) = self.have_heartbeats.iter().position(|c| *c == client_id) {
            self.have_heartbeats.swap_remove(pos);
        }
    }
}

pub struct Server {
    writers: Arc<Mutex<HashMap<Id, Arc<Mutex<OwnedWriteHalf>>>>>,
    state: Arc<Mutex<ServerState>>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            writers: Arc::new(Mutex::new(HashMap::new())),
            state: Arc::new(Mutex::new(ServerState::new())),
        }
    }

    async fn parse_plate(
        stream: &mut OwnedReadHalf,
        id: Id,
        buffer: &mut Vec<u8>,
    ) -> Option<Plate> {
        let plate_len = utils::read_for(stream, buffer, 1).await?[0] as usize;
        let plate = utils::read_for(stream, buffer, plate_len).await?;
        let plate = String::from_utf8_lossy(&plate).into_owned();
        let timestamp = utils::read_for(stream, buffer, 4).await?;
        let timestamp = u32::from_be_bytes(timestamp.try_into().unwrap());
        Some(Plate {
            id,
            plate,
            timestamp,
        })
    }

    async fn parse_heartbeat(
        stream: &mut OwnedReadHalf,
        id: Id,
        buffer: &mut Vec<u8>,
    ) -> Option<Heartbeat> {
        let interval = utils::read_for(stream, buffer, 4).await?;
        let interval = u32::from_be_bytes(interval.try_into().unwrap());
        Some(Heartbeat { id, interval })
    }

    async fn parse_camera(
        stream: &mut OwnedReadHalf,
        id: Id,
        buffer: &mut Vec<u8>,
    ) -> Option<Camera> {
        let road = utils::read_for(stream, buffer, 2).await?;
        let road = u16::from_be_bytes(road.try_into().unwrap());
        let mile = utils::read_for(stream, buffer, 2).await?;
        let mile = u16::from_be_bytes(mile.try_into().unwrap());
        let limit = utils::read_for(stream, buffer, 2).await?;
        let limit = u16::from_be_bytes(limit.try_into().unwrap());
        Some(Camera {
            id,
            road,
            mile,
            limit,
        })
    }

    async fn parse_dispatcher(
        stream: &mut OwnedReadHalf,
        id: Id,
        buffer: &mut Vec<u8>,
    ) -> Option<Dispatcher> {
        let numroads = utils::read_for(stream, buffer, 1).await?[0];
        let mut roads = Vec::new();
        for _ in 0..numroads {
            let road = utils::read_for(stream, buffer, 2).await?;
            let road = u16::from_be_bytes(road.try_into().unwrap());
            roads.push(road);
        }
        Some(Dispatcher { id, roads })
    }

    async fn get_client_id(&self) -> Id {
        let clients = self.writers.lock().await;
        let clients_id = clients.keys().cloned().collect::<Vec<_>>();
        drop(clients);
        for i in 0.. {
            if !clients_id.contains(&i) {
                return i;
            }
        }
        unreachable!()
    }

    async fn add_client(&self, client_id: Id, writer: Arc<Mutex<OwnedWriteHalf>>) {
        self.writers.lock().await.insert(client_id, writer);
    }

    async fn process_request(
        &self,
        client_id: Id,
        reader: &mut OwnedReadHalf,
        buffer: &mut Vec<u8>,
    ) -> ServerResult {
        let Some(msg_type) = utils::read_for(reader, buffer, 1).await else {
            return Err("could not receive message type");
        };
        let msg_type = msg_type[0];

        match msg_type {
            0x20 => {
                let plate = Self::parse_plate(reader, client_id, buffer)
                    .await
                    .ok_or("error when parsing plate")?;
                self.state.lock().await.read_plate(plate)
            }
            0x40 => {
                let heartbeat = Self::parse_heartbeat(reader, client_id, buffer)
                    .await
                    .ok_or("error when parsing heartbeat")?;
                self.state.lock().await.mark_heartbeat(heartbeat)
            }
            0x80 => {
                let camera = Self::parse_camera(reader, client_id, buffer)
                    .await
                    .ok_or("error when parsing camera")?;
                self.state.lock().await.add_camera(camera)
            }
            0x81 => {
                let dispatcher = Self::parse_dispatcher(reader, client_id, buffer)
                    .await
                    .ok_or("error when parsing dispatcher")?;
                self.state.lock().await.add_dispatcher(dispatcher)
            }
            _ => Err("invalid message type"),
        }
    }

    async fn process_msg(&self, msg: ServerMessage, writer: &Arc<Mutex<OwnedWriteHalf>>) {
        match msg {
            ServerMessage::WantHeartbeat { interval } => {
                if interval > 0 {
                    let writer = Arc::clone(&writer);
                    tokio::spawn(async move { Self::send_heartbeat(writer, interval).await });
                }
            }

            ServerMessage::Ticket {
                recipient,
                plate,
                road,
                mile1,
                timestamp1,
                mile2,
                timestamp2,
                speed,
            } => {
                let mut ticket_data = vec![0x21];
                ticket_data.push(plate.len() as u8);
                ticket_data.extend_from_slice(plate.as_bytes());
                ticket_data.extend(road.to_be_bytes());
                ticket_data.extend(mile1.to_be_bytes());
                ticket_data.extend(timestamp1.to_be_bytes());
                ticket_data.extend(mile2.to_be_bytes());
                ticket_data.extend(timestamp2.to_be_bytes());
                ticket_data.extend(speed.to_be_bytes());
                self.send_to(recipient.unwrap(), ticket_data).await;
            }
        }
    }

    async fn send_to(&self, client_id: Id, data: Vec<u8>) {
        let mut client_map = self.writers.lock().await;
        let writer = client_map.get_mut(&client_id).unwrap();
        let _ = writer.lock().await.write_all(&data).await;
    }

    async fn send_heartbeat(writer: Arc<Mutex<OwnedWriteHalf>>, interval: u32) {
        let mut interval = time::interval(time::Duration::from_millis((100 * interval).into()));
        let heartbeat = Vec::from([0x41]);
        loop {
            interval.tick().await;
            let mut writer = writer.lock().await;
            if let Err(_) = writer.write_all(&heartbeat).await {
                break;
            }
        }
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, stream: TcpStream) {
        let (mut reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let client_id = self.get_client_id().await;
        self.add_client(client_id, Arc::clone(&writer)).await;
        let mut buffer = Vec::new();
        loop {
            match self
                .process_request(client_id, &mut reader, &mut buffer)
                .await
            {
                Ok(message_list) => {
                    for msg in message_list {
                        self.process_msg(msg, &writer).await;
                    }
                }
                Err(err_msg) => {
                    let err_data =
                        [vec![0x10, err_msg.len() as u8], err_msg.as_bytes().to_vec()].concat();
                    self.send_to(client_id, err_data).await;
                    break;
                }
            };
        }

        self.state.lock().await.remove_client(client_id);
    }
}
