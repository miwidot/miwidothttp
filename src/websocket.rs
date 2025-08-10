use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Path, Query,
    },
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WebSocketManager {
    connections: Arc<RwLock<HashMap<String, WebSocketConnection>>>,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    rooms: Arc<RwLock<HashMap<String, Room>>>,
}

#[derive(Debug)]
struct WebSocketConnection {
    id: String,
    user_id: Option<String>,
    room_id: Option<String>,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct Room {
    id: String,
    name: String,
    members: Vec<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastMessage {
    pub msg_type: MessageType,
    pub sender_id: String,
    pub room_id: Option<String>,
    pub data: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Text,
    Binary,
    Ping,
    Pong,
    Join,
    Leave,
    Broadcast,
    Private,
    RoomMessage,
    SystemNotification,
    Error,
}

impl WebSocketManager {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(1000);
        
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn handle_upgrade(
        &self,
        ws: WebSocketUpgrade,
        user_agent: Option<String>,
    ) -> Response {
        let manager = self.clone();
        
        ws.on_upgrade(move |socket| async move {
            if let Err(e) = manager.handle_socket(socket, user_agent).await {
                error!("WebSocket error: {}", e);
            }
        })
    }
    
    async fn handle_socket(
        &self,
        socket: WebSocket,
        user_agent: Option<String>,
    ) -> Result<()> {
        let conn_id = Uuid::new_v4().to_string();
        info!("New WebSocket connection: {} (UA: {:?})", conn_id, user_agent);
        
        // Register connection
        let connection = WebSocketConnection {
            id: conn_id.clone(),
            user_id: None,
            room_id: None,
            metadata: HashMap::new(),
        };
        
        self.connections.write().await.insert(conn_id.clone(), connection);
        
        // Split the WebSocket
        let (mut sender, mut receiver) = socket.split();
        
        // Subscribe to broadcasts
        let mut broadcast_rx = self.broadcast_tx.subscribe();
        let conn_id_clone = conn_id.clone();
        
        // Spawn task to handle broadcasts
        let broadcast_task = tokio::spawn(async move {
            while let Ok(msg) = broadcast_rx.recv().await {
                // Filter messages based on room membership
                let should_send = msg.room_id.is_none() || {
                    // Check if connection is in the target room
                    true // TODO: Implement room filtering
                };
                
                if should_send && msg.sender_id != conn_id_clone {
                    let json = serde_json::to_string(&msg).unwrap();
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
            }
        });
        
        // Handle incoming messages
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_text_message(&conn_id, text).await?;
                }
                Ok(Message::Binary(data)) => {
                    self.handle_binary_message(&conn_id, data).await?;
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping from {}", conn_id);
                    // Axum handles pong automatically
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong from {}", conn_id);
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket {} closing", conn_id);
                    break;
                }
                Err(e) => {
                    error!("WebSocket error for {}: {}", conn_id, e);
                    break;
                }
            }
        }
        
        // Cleanup
        broadcast_task.abort();
        self.connections.write().await.remove(&conn_id);
        self.broadcast_leave(&conn_id).await?;
        
        info!("WebSocket {} disconnected", conn_id);
        Ok(())
    }
    
    async fn handle_text_message(&self, conn_id: &str, text: String) -> Result<()> {
        // Parse JSON message
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(msg) => {
                match msg.action.as_str() {
                    "join_room" => {
                        if let Some(room_id) = msg.room_id {
                            self.join_room(conn_id, &room_id).await?;
                        }
                    }
                    "leave_room" => {
                        if let Some(room_id) = msg.room_id {
                            self.leave_room(conn_id, &room_id).await?;
                        }
                    }
                    "broadcast" => {
                        self.broadcast_message(conn_id, msg.data).await?;
                    }
                    "private_message" => {
                        if let Some(target_id) = msg.target_id {
                            self.send_private_message(conn_id, &target_id, msg.data).await?;
                        }
                    }
                    _ => {
                        warn!("Unknown action: {}", msg.action);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to parse message from {}: {}", conn_id, e);
            }
        }
        
        Ok(())
    }
    
    async fn handle_binary_message(&self, conn_id: &str, data: Vec<u8>) -> Result<()> {
        debug!("Received {} bytes of binary data from {}", data.len(), conn_id);
        
        // Broadcast binary data
        let msg = BroadcastMessage {
            msg_type: MessageType::Binary,
            sender_id: conn_id.to_string(),
            room_id: None,
            data: serde_json::json!({
                "size": data.len(),
                "data": base64::encode(&data),
            }),
            timestamp: chrono::Utc::now(),
        };
        
        let _ = self.broadcast_tx.send(msg);
        Ok(())
    }
    
    async fn join_room(&self, conn_id: &str, room_id: &str) -> Result<()> {
        let mut rooms = self.rooms.write().await;
        let room = rooms.entry(room_id.to_string()).or_insert_with(|| Room {
            id: room_id.to_string(),
            name: format!("Room {}", room_id),
            members: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        
        if !room.members.contains(&conn_id.to_string()) {
            room.members.push(conn_id.to_string());
            info!("{} joined room {}", conn_id, room_id);
            
            // Update connection
            if let Some(conn) = self.connections.write().await.get_mut(conn_id) {
                conn.room_id = Some(room_id.to_string());
            }
            
            // Broadcast join message
            let msg = BroadcastMessage {
                msg_type: MessageType::Join,
                sender_id: conn_id.to_string(),
                room_id: Some(room_id.to_string()),
                data: serde_json::json!({
                    "user_id": conn_id,
                    "room_id": room_id,
                }),
                timestamp: chrono::Utc::now(),
            };
            
            let _ = self.broadcast_tx.send(msg);
        }
        
        Ok(())
    }
    
    async fn leave_room(&self, conn_id: &str, room_id: &str) -> Result<()> {
        let mut rooms = self.rooms.write().await;
        
        if let Some(room) = rooms.get_mut(room_id) {
            room.members.retain(|id| id != conn_id);
            info!("{} left room {}", conn_id, room_id);
            
            // Remove room if empty
            if room.members.is_empty() {
                rooms.remove(room_id);
                info!("Room {} removed (empty)", room_id);
            }
        }
        
        // Update connection
        if let Some(conn) = self.connections.write().await.get_mut(conn_id) {
            conn.room_id = None;
        }
        
        Ok(())
    }
    
    async fn broadcast_message(&self, sender_id: &str, data: serde_json::Value) -> Result<()> {
        let connections = self.connections.read().await;
        let sender_room = connections.get(sender_id).and_then(|c| c.room_id.clone());
        
        let msg = BroadcastMessage {
            msg_type: MessageType::Broadcast,
            sender_id: sender_id.to_string(),
            room_id: sender_room,
            data,
            timestamp: chrono::Utc::now(),
        };
        
        let _ = self.broadcast_tx.send(msg);
        Ok(())
    }
    
    async fn send_private_message(
        &self,
        sender_id: &str,
        target_id: &str,
        data: serde_json::Value,
    ) -> Result<()> {
        let msg = BroadcastMessage {
            msg_type: MessageType::Private,
            sender_id: sender_id.to_string(),
            room_id: Some(target_id.to_string()), // Use room_id as target
            data,
            timestamp: chrono::Utc::now(),
        };
        
        let _ = self.broadcast_tx.send(msg);
        Ok(())
    }
    
    async fn broadcast_leave(&self, conn_id: &str) -> Result<()> {
        let msg = BroadcastMessage {
            msg_type: MessageType::Leave,
            sender_id: conn_id.to_string(),
            room_id: None,
            data: serde_json::json!({
                "user_id": conn_id,
            }),
            timestamp: chrono::Utc::now(),
        };
        
        let _ = self.broadcast_tx.send(msg);
        Ok(())
    }
    
    pub async fn get_connection_count(&self) -> usize {
        self.connections.read().await.len()
    }
    
    pub async fn get_room_count(&self) -> usize {
        self.rooms.read().await.len()
    }
    
    pub async fn get_rooms(&self) -> Vec<RoomInfo> {
        let rooms = self.rooms.read().await;
        rooms.values().map(|r| RoomInfo {
            id: r.id.clone(),
            name: r.name.clone(),
            member_count: r.members.len(),
            created_at: r.created_at,
        }).collect()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientMessage {
    action: String,
    room_id: Option<String>,
    target_id: Option<String>,
    data: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct RoomInfo {
    pub id: String,
    pub name: String,
    pub member_count: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// WebSocket routes
pub fn websocket_routes() -> axum::Router {
    use axum::routing::get;
    
    let manager = Arc::new(WebSocketManager::new());
    
    axum::Router::new()
        .route("/ws", get(websocket_handler))
        .route("/ws/stats", get(websocket_stats))
        .with_state(manager)
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(manager): State<Arc<WebSocketManager>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    manager.handle_upgrade(ws, user_agent).await
}

async fn websocket_stats(
    State(manager): State<Arc<WebSocketManager>>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "connections": manager.get_connection_count().await,
        "rooms": manager.get_room_count().await,
        "room_list": manager.get_rooms().await,
    }))
}