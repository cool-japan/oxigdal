//! JavaScript client SDK generation

use crate::client_sdk::ClientSdkConfig;

/// Generate JavaScript client SDK
pub fn generate_javascript_client(config: &ClientSdkConfig) -> String {
    let reconnection_code = if config.enable_reconnection {
        format!(
            r#"
    reconnect() {{
        if (this.reconnectAttempts >= {max_attempts}) {{
            console.error('Max reconnection attempts reached');
            this.emit('maxReconnectAttemptsReached');
            return;
        }}

        this.reconnectAttempts++;
        const delay = {delay} * Math.pow(2, this.reconnectAttempts - 1);

        console.log(`Reconnecting in ${{delay}}ms (attempt ${{this.reconnectAttempts}})`);

        setTimeout(() => {{
            this.connect(this.url);
        }}, delay);
    }}"#,
            max_attempts = config.max_reconnection_attempts,
            delay = config.reconnection_delay_ms
        )
    } else {
        "    // Reconnection disabled".to_string()
    };

    let caching_code = if config.enable_caching {
        r#"
    initCache() {
        this.cache = {
            tiles: new Map(),
            features: new Map(),
        };
    }

    getCachedTile(z, x, y) {
        const key = `${z}/${x}/${y}`;
        return this.cache.tiles.get(key);
    }

    cacheTile(z, x, y, data) {
        const key = `${z}/${x}/${y}`;
        this.cache.tiles.set(key, data);
    }

    getCachedFeature(layer, id) {
        const key = `${layer}:${id}`;
        return this.cache.features.get(key);
    }

    cacheFeature(layer, id, feature) {
        const key = `${layer}:${id}`;
        this.cache.features.set(key, feature);
    }

    clearCache() {
        this.cache.tiles.clear();
        this.cache.features.clear();
    }"#
    } else {
        "    // Caching disabled"
    };

    format!(
        r#"/**
 * OxiGDAL WebSocket Client
 *
 * A JavaScript client for connecting to OxiGDAL WebSocket server
 * with support for real-time geospatial data updates.
 */

class OxiGDALWebSocketClient {{
    constructor(options = {{}}) {{
        this.url = null;
        this.ws = null;
        this.connected = false;
        this.reconnectAttempts = 0;
        this.maxReconnectAttempts = {max_reconnect};
        this.reconnectDelay = {reconnect_delay};
        this.subscriptions = new Map();
        this.rooms = new Set();
        this.messageHandlers = new Map();
        this.eventEmitter = new EventTarget();

        {init_cache}
    }}

    /**
     * Connect to the WebSocket server
     */
    connect(url) {{
        this.url = url;

        try {{
            this.ws = new WebSocket(url);

            this.ws.onopen = () => {{
                console.log('WebSocket connected');
                this.connected = true;
                this.reconnectAttempts = 0;
                this.emit('connected');
            }};

            this.ws.onmessage = (event) => {{
                this.handleMessage(event.data);
            }};

            this.ws.onerror = (error) => {{
                console.error('WebSocket error:', error);
                this.emit('error', error);
            }};

            this.ws.onclose = () => {{
                console.log('WebSocket disconnected');
                this.connected = false;
                this.emit('disconnected');

                if (this.reconnectAttempts < this.maxReconnectAttempts) {{
                    this.reconnect();
                }}
            }};
        }} catch (error) {{
            console.error('Failed to connect:', error);
            this.emit('error', error);
        }}
    }}

    /**
     * Disconnect from the server
     */
    disconnect() {{
        if (this.ws) {{
            this.ws.close();
            this.ws = null;
        }}
        this.connected = false;
    }}

    /**
     * Send a message to the server
     */
    send(message) {{
        if (!this.connected || !this.ws) {{
            console.error('Not connected');
            return false;
        }}

        try {{
            const data = JSON.stringify(message);
            this.ws.send(data);
            return true;
        }} catch (error) {{
            console.error('Failed to send message:', error);
            return false;
        }}
    }}

    /**
     * Subscribe to a topic
     */
    subscribe(topic, handler) {{
        if (!this.subscriptions.has(topic)) {{
            this.subscriptions.set(topic, new Set());
        }}
        this.subscriptions.get(topic).add(handler);

        // Send subscribe message to server
        this.send({{
            msg_type: 'Subscribe',
            payload: {{
                Subscribe: {{
                    topic: topic,
                    filter: null
                }}
            }}
        }});

        return () => this.unsubscribe(topic, handler);
    }}

    /**
     * Unsubscribe from a topic
     */
    unsubscribe(topic, handler) {{
        if (this.subscriptions.has(topic)) {{
            this.subscriptions.get(topic).delete(handler);

            if (this.subscriptions.get(topic).size === 0) {{
                this.subscriptions.delete(topic);

                // Send unsubscribe message to server
                this.send({{
                    msg_type: 'Unsubscribe',
                    payload: {{
                        Subscribe: {{
                            topic: topic,
                            filter: null
                        }}
                    }}
                }});
            }}
        }}
    }}

    /**
     * Join a room
     */
    joinRoom(roomName) {{
        this.rooms.add(roomName);

        this.send({{
            msg_type: 'JoinRoom',
            payload: {{
                Room: {{
                    room: roomName
                }}
            }}
        }});
    }}

    /**
     * Leave a room
     */
    leaveRoom(roomName) {{
        this.rooms.delete(roomName);

        this.send({{
            msg_type: 'LeaveRoom',
            payload: {{
                Room: {{
                    room: roomName
                }}
            }}
        }});
    }}

    /**
     * Handle incoming message
     */
    handleMessage(data) {{
        try {{
            const message = JSON.parse(data);

            // Handle based on message type
            switch (message.msg_type) {{
                case 'TileUpdate':
                    this.handleTileUpdate(message);
                    break;
                case 'FeatureUpdate':
                    this.handleFeatureUpdate(message);
                    break;
                case 'ChangeStream':
                    this.handleChangeStream(message);
                    break;
                case 'Pong':
                    this.emit('pong');
                    break;
                default:
                    this.emit('message', message);
            }}
        }} catch (error) {{
            console.error('Failed to handle message:', error);
        }}
    }}

    /**
     * Handle tile update
     */
    handleTileUpdate(message) {{
        const tile = message.payload.TileData;

        {cache_tile}

        this.emit('tileUpdate', tile);
    }}

    /**
     * Handle feature update
     */
    handleFeatureUpdate(message) {{
        const feature = message.payload.FeatureData;

        {cache_feature}

        this.emit('featureUpdate', feature);
    }}

    /**
     * Handle change stream event
     */
    handleChangeStream(message) {{
        const change = message.payload.ChangeEvent;
        this.emit('changeStream', change);
    }}

    /**
     * Send ping to server
     */
    ping() {{
        this.send({{
            msg_type: 'Ping',
            payload: 'Empty'
        }});
    }}

    /**
     * Emit an event
     */
    emit(eventName, data) {{
        const event = new CustomEvent(eventName, {{ detail: data }});
        this.eventEmitter.dispatchEvent(event);
    }}

    /**
     * Add event listener
     */
    on(eventName, handler) {{
        this.eventEmitter.addEventListener(eventName, (e) => handler(e.detail));
    }}

    /**
     * Remove event listener
     */
    off(eventName, handler) {{
        this.eventEmitter.removeEventListener(eventName, handler);
    }}

{reconnection_code}

{caching_code}
}}

// Export for use in Node.js and browsers
if (typeof module !== 'undefined' && module.exports) {{
    module.exports = OxiGDALWebSocketClient;
}}
"#,
        max_reconnect = config.max_reconnection_attempts,
        reconnect_delay = config.reconnection_delay_ms,
        init_cache = if config.enable_caching {
            "this.initCache();"
        } else {
            ""
        },
        cache_tile = if config.enable_caching {
            "this.cacheTile(tile.z, tile.x, tile.y, tile);"
        } else {
            ""
        },
        cache_feature = if config.enable_caching {
            "this.cacheFeature(feature.layer, feature.id, feature);"
        } else {
            ""
        },
        reconnection_code = reconnection_code,
        caching_code = caching_code,
    )
}

/// Generate TypeScript definitions
pub fn generate_typescript_definitions() -> String {
    r#"/**
 * OxiGDAL WebSocket Client TypeScript Definitions
 */

export interface OxiGDALClientOptions {
    reconnectAttempts?: number;
    reconnectDelay?: number;
}

export interface Message {
    id: string;
    msg_type: MessageType;
    timestamp: number;
    payload: Payload;
    correlation_id?: string;
}

export type MessageType =
    | 'Ping'
    | 'Pong'
    | 'Subscribe'
    | 'Unsubscribe'
    | 'Publish'
    | 'Data'
    | 'TileUpdate'
    | 'FeatureUpdate'
    | 'ChangeStream'
    | 'Error'
    | 'Ack'
    | 'JoinRoom'
    | 'LeaveRoom'
    | 'Broadcast'
    | 'SystemEvent';

export type Payload = any; // Can be refined based on message type

export interface TileData {
    z: number;
    x: number;
    y: number;
    data: Uint8Array;
    format: string;
    delta?: Uint8Array;
}

export interface FeatureData {
    id: string;
    layer: string;
    feature: GeoJSON.Feature;
    change_type: ChangeType;
}

export type ChangeType = 'Created' | 'Updated' | 'Deleted';

export interface ChangeEvent {
    change_id: number;
    collection: string;
    change_type: ChangeType;
    document_id: string;
    data?: any;
}

export declare class OxiGDALWebSocketClient {
    constructor(options?: OxiGDALClientOptions);

    connect(url: string): void;
    disconnect(): void;
    send(message: Message): boolean;

    subscribe(topic: string, handler: (data: any) => void): () => void;
    unsubscribe(topic: string, handler: (data: any) => void): void;

    joinRoom(roomName: string): void;
    leaveRoom(roomName: string): void;

    ping(): void;

    on(eventName: string, handler: (data: any) => void): void;
    off(eventName: string, handler: (data: any) => void): void;

    getCachedTile?(z: number, x: number, y: number): TileData | undefined;
    cacheTile?(z: number, x: number, y: number, data: TileData): void;
    getCachedFeature?(layer: string, id: string): FeatureData | undefined;
    cacheFeature?(layer: string, id: string, feature: FeatureData): void;
    clearCache?(): void;
}

export default OxiGDALWebSocketClient;
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_javascript_client() {
        let config = ClientSdkConfig::default();
        let js_code = generate_javascript_client(&config);

        assert!(js_code.contains("OxiGDALWebSocketClient"));
        assert!(js_code.contains("connect"));
        assert!(js_code.contains("subscribe"));
        assert!(js_code.contains("joinRoom"));
    }

    #[test]
    fn test_generate_javascript_client_no_reconnection() {
        let config = ClientSdkConfig {
            enable_reconnection: false,
            ..Default::default()
        };
        let js_code = generate_javascript_client(&config);

        assert!(js_code.contains("Reconnection disabled"));
    }

    #[test]
    fn test_generate_javascript_client_no_caching() {
        let config = ClientSdkConfig {
            enable_caching: false,
            ..Default::default()
        };
        let js_code = generate_javascript_client(&config);

        assert!(js_code.contains("Caching disabled"));
    }

    #[test]
    fn test_generate_typescript_definitions() {
        let ts_defs = generate_typescript_definitions();

        assert!(ts_defs.contains("OxiGDALWebSocketClient"));
        assert!(ts_defs.contains("MessageType"));
        assert!(ts_defs.contains("TileData"));
        assert!(ts_defs.contains("FeatureData"));
    }
}
