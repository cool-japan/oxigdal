//! WebSocket integration tests.

use oxigdal_gateway::websocket::{Connection, WebSocketManager, WsMessage};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_websocket_manager_creation() {
    let manager = WebSocketManager::new();
    assert_eq!(manager.connection_count(), 0);
}

#[tokio::test]
async fn test_connection_registration() {
    let manager = WebSocketManager::new();
    let conn = Connection::new("conn_123".to_string());
    let (sender, _receiver) = mpsc::unbounded_channel();

    let result = manager.register_connection(conn, sender);
    assert!(result.is_ok());
    assert_eq!(manager.connection_count(), 1);
}

#[tokio::test]
async fn test_send_message_to_connection() {
    let manager = WebSocketManager::new();
    let conn = Connection::new("conn_456".to_string());
    let (sender, mut receiver) = mpsc::unbounded_channel();

    let _ = manager.register_connection(conn, sender);

    let message = WsMessage::Text("Hello WebSocket".to_string());
    let result = manager.send_to_connection("conn_456", message);
    assert!(result.is_ok());

    let received = receiver.try_recv();
    assert!(received.is_ok());
}

#[tokio::test]
async fn test_broadcast_message() {
    let manager = WebSocketManager::new();

    // Register multiple connections
    // Keep receivers alive to prevent channels from being closed
    let mut _receivers = Vec::new();
    for i in 0..3 {
        let conn = Connection::new(format!("conn_{}", i));
        let (sender, receiver) = mpsc::unbounded_channel();
        _receivers.push(receiver);
        let _ = manager.register_connection(conn, sender);
    }

    let message = WsMessage::Text("Broadcast message".to_string());
    let result = manager.broadcast(message);

    assert!(result.is_ok());
    assert_eq!(result.ok(), Some(3));
}

#[tokio::test]
async fn test_user_connections() {
    let manager = WebSocketManager::new();

    // Register connections for user1
    for i in 0..2 {
        let mut conn = Connection::new(format!("conn_user1_{}", i));
        conn.user_id = Some("user1".to_string());
        let (sender, _) = mpsc::unbounded_channel();
        let _ = manager.register_connection(conn, sender);
    }

    // Register connections for user2
    let mut conn = Connection::new("conn_user2_0".to_string());
    conn.user_id = Some("user2".to_string());
    let (sender, _) = mpsc::unbounded_channel();
    let _ = manager.register_connection(conn, sender);

    let user1_conns = manager.get_user_connections("user1");
    assert_eq!(user1_conns.len(), 2);

    let user2_conns = manager.get_user_connections("user2");
    assert_eq!(user2_conns.len(), 1);
}

#[tokio::test]
async fn test_connection_cleanup() {
    let manager = WebSocketManager::new();
    let conn = Connection::new("conn_cleanup".to_string());
    let (sender, _) = mpsc::unbounded_channel();

    let _ = manager.register_connection(conn, sender);
    assert_eq!(manager.connection_count(), 1);

    let _ = manager.unregister_connection("conn_cleanup");
    assert_eq!(manager.connection_count(), 0);
}
