use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Notify};

use crate::{utils, TcpServer};

type ClientId = u64;
type JobId = u64;

struct Job {
    id: JobId,
    queue: String,
    priority: u64,
    task: Value,
}

enum ServerMessage {
    Response(String),
    Waiting,
    Notify(ClientId),
}

struct ServerState {
    next_job_id: JobId,
    client_jobs: HashMap<ClientId, Vec<Job>>,
    queues: HashMap<String, Vec<Job>>,
    waiting_clients: HashMap<ClientId, Vec<String>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            next_job_id: 1,
            client_jobs: HashMap::new(),
            queues: HashMap::new(),
            waiting_clients: HashMap::new(),
        }
    }

    fn generate_error(&self, err_msg: &str) -> Vec<ServerMessage> {
        vec![ServerMessage::Response(
            json!({
                "status": "error",
                "error": err_msg,
            })
            .to_string(),
        )]
    }

    fn put(&mut self, mut request: Value) -> Vec<ServerMessage> {
        let Value::String(queue) = request["queue"].take() else {
            return self.generate_error("invalid key 'queue' in put request");
        };

        let Value::Number(priority) = request["pri"].take() else {
            return self.generate_error("invalid key 'priority' in put request");
        };

        let Some(priority) = priority.as_u64() else {
            return self.generate_error(&format!("invalid priority '{priority}' in put request"));
        };

        let task = match request["job"].take() {
            Value::Null => return self.generate_error("key 'job' not found in put request"),
            val => val,
        };

        let job_id = self.next_job_id;
        self.next_job_id += 1;

        let new_job = Job {
            id: job_id,
            queue: queue.to_string(),
            priority,
            task,
        };

        let mut responses = vec![ServerMessage::Response(
            json!({"status": "ok", "id": job_id}).to_string(),
        )];

        for (&client_id, waiting_queues) in &self.waiting_clients {
            if waiting_queues.contains(&queue) {
                responses.push(ServerMessage::Notify(client_id));
                break;
            }
        }

        self.queues
            .entry(queue)
            .or_insert_with(Vec::new)
            .push(new_job);

        responses
    }

    fn get(&mut self, client_id: ClientId, mut request: Value) -> Vec<ServerMessage> {
        let Value::Array(queues) = request["queues"].take() else {
            return self.generate_error("key 'queues' is not an array");
        };

        let Ok(queues) = queues
            .into_iter()
            .map(|queue| match queue {
                Value::String(s) => Ok(s),
                _ => Err(0),
            })
            .collect::<Result<Vec<_>, _>>()
        else {
            return self.generate_error("invalid queue type in 'queues'");
        };

        let mut highest_prio = 0u64;
        let mut highest_prio_queue = None;

        for queue in &queues {
            if !self.queues.contains_key(queue) {
                continue;
            }

            let Some(prio) = self.queues[queue].iter().map(|job| job.priority).max() else {
                continue;
            };
            if prio > highest_prio {
                highest_prio = prio;
                highest_prio_queue = Some(queue);
            }
        }

        if let Some(highest_queue) = highest_prio_queue {
            let queue = self.queues.get_mut(highest_queue).unwrap();
            let index = queue
                .iter()
                .position(|job| job.priority == highest_prio)
                .unwrap();
            let job = queue.remove(index);

            let response = json!({"status": "ok", "id": job.id, "pri": job.priority, "queue": highest_queue, "job": job.task});
            self.client_jobs
                .entry(client_id)
                .or_insert_with(Vec::new)
                .push(job);

            vec![ServerMessage::Response(response.to_string())]
        } else {
            if request
                .get("wait")
                .is_some_and(|v| v.as_bool().unwrap_or(false))
            {
                self.waiting_clients.insert(client_id, queues);
                vec![ServerMessage::Waiting]
            } else {
                vec![ServerMessage::Response(
                    json!({"status": "no-job"}).to_string(),
                )]
            }
        }
    }

    fn delete(&mut self, mut request: Value) -> Vec<ServerMessage> {
        let Value::Number(job_id) = request["id"].take() else {
            return self.generate_error("invalid id");
        };
        let job_id = job_id.as_u64().unwrap();

        let mut job_removed = false;
        for (_, jobs) in &mut self.queues {
            if let Some(index) = jobs.iter().position(|job| job.id == job_id) {
                job_removed = true;
                jobs.swap_remove(index);
            }
        }
        for (_, jobs) in &mut self.client_jobs {
            let n = jobs.len();
            jobs.retain(|job| job.id != job_id);
            job_removed = job_removed || (jobs.len() < n);
        }

        let response = json!({"status": if job_removed { "ok" } else {"no-job"}});
        vec![ServerMessage::Response(response.to_string())]
    }

    fn abort(&mut self, client_id: ClientId, mut request: Value) -> Vec<ServerMessage> {
        let Value::Number(job_id) = request["id"].take() else {
            return self.generate_error("invalid id");
        };
        let job_id = job_id.as_u64().unwrap();

        let Some(jobs) = self.client_jobs.get_mut(&client_id) else {
            return vec![ServerMessage::Response(
                json!({"status": "no-job"}).to_string(),
            )];
        };

        let Some(job_index) = jobs.iter().position(|job| job.id == job_id) else {
            return self.generate_error(&format!("client not working on job '{job_id}'"));
        };

        let job = jobs.swap_remove(job_index);

        let mut responses = vec![ServerMessage::Response(json!({"status": "ok"}).to_string())];
        for (&client_id, waiting_queues) in &self.waiting_clients {
            if waiting_queues.contains(&job.queue) {
                responses.push(ServerMessage::Notify(client_id));
                break;
            }
        }

        self.queues
            .entry(job.queue.clone())
            .or_insert_with(Vec::new)
            .push(job);

        responses
    }

    fn get_responses(&mut self, client_id: ClientId, request: &str) -> Vec<ServerMessage> {
        let Ok(request): Result<Value, serde_json::Error> = serde_json::from_str(request) else {
            return self.generate_error("invalid JSON format");
        };

        let Some(request_type) = request.get("request") else {
            return self.generate_error("key 'request' not found in JSON");
        };

        match request_type {
            Value::String(val) if val == "put" => self.put(request),
            Value::String(val) if val == "get" => self.get(client_id, request),
            Value::String(val) if val == "delete" => self.delete(request),
            Value::String(val) if val == "abort" => self.abort(client_id, request),
            Value::String(val) => self.generate_error(&format!("invalid request type: '{val}'")),
            _ => self.generate_error("key 'request' is not a string"),
        }
    }

    fn disconnect(&mut self, client_id: ClientId) -> Vec<ServerMessage> {
        let Some(jobs) = self.client_jobs.remove(&client_id) else {
            return Vec::new();
        };

        let mut responses = Vec::new();
        for job in jobs {
            for (&client_to_wake, waiting_queues) in &self.waiting_clients {
                if waiting_queues.contains(&job.queue) {
                    responses.push(ServerMessage::Notify(client_to_wake));
                    break;
                }
            }

            self.queues
                .entry(job.queue.clone())
                .or_insert_with(Vec::new)
                .push(job);
        }

        responses
    }
}

pub struct Server {
    next_client_id: Arc<Mutex<ClientId>>,
    state: Arc<Mutex<ServerState>>,
    waiting: Arc<Mutex<HashMap<ClientId, Arc<Notify>>>>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            next_client_id: Arc::new(Mutex::new(0)),
            state: Arc::new(Mutex::new(ServerState::new())),
            waiting: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn get_client_id(&self) -> ClientId {
        let mut next_client_id = self.next_client_id.lock().await;
        *next_client_id += 1;
        *next_client_id
    }

    async fn wait_job(&self, client_id: ClientId) {
        let event = Arc::new(Notify::new());
        let event2 = Arc::clone(&event);
        let mut waiting = self.waiting.lock().await;
        waiting.insert(client_id, event);
        drop(waiting);
        event2.notified().await;
    }

    async fn notify(&self, client_id: ClientId) {
        let mut waiting = self.waiting.lock().await;
        waiting
            .remove(&client_id)
            .inspect(|event| event.notify_one());
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = [0; 1024];

        let client_id = self.get_client_id().await;
        println!("Client {client_id} connected!");

        while let Some(request) = utils::read_until(&mut stream, &mut buffer, '\n').await {
            println!("<--- [{client_id}] {request}");
            let mut should_wait = true;
            while should_wait {
                should_wait = false;

                let mut state = self.state.lock().await;
                let responses = state.get_responses(client_id, &request);
                drop(state);
                for response in responses {
                    match response {
                        ServerMessage::Waiting => should_wait = true,
                        ServerMessage::Notify(client_to_wake) => self.notify(client_to_wake).await,
                        ServerMessage::Response(mut value) => {
                            println!("---> [{client_id}] {value}");
                            value.push('\n');
                            let _ = stream.write_all(value.as_bytes()).await;
                        }
                    }
                }

                if should_wait {
                    self.wait_job(client_id).await;
                }
            }
        }

        println!("Client {client_id} disconnected!");
        let mut state = self.state.lock().await;
        let responses = state.disconnect(client_id);
        for response in responses {
            match response {
                ServerMessage::Notify(client_to_wake) => self.notify(client_to_wake).await,
                _ => unreachable!(),
            }
        }
    }
}
