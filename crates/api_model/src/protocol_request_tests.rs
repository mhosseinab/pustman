use crate::{
    Auth, DescriptorSource, GraphQlRequest, GrpcRequest, ItemId, KeyValue, MessageKind, Request,
    RequestSettings, SavedEmit, SavedMessage, SocketIoRequest, WebSocketRequest,
};
use std::collections::BTreeMap;

fn header(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// Serialize a request, check its `type` tag, decode it back and return the decoded value.
fn round_trip(request: &Request, tag: &str) -> Request {
    let json = serde_json::to_value(request).unwrap();
    assert_eq!(json["type"], tag, "the request carries its kind tag");
    let bytes = serde_json::to_vec(&json).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn graphql_request_round_trips_json() {
    let request = Request::GraphQl(GraphQlRequest {
        id: ItemId::new(),
        name: "Search users".to_string(),
        url: "https://api.example.test/graphql".to_string(),
        headers: vec![header("X-Trace", "on")],
        auth: Auth::Bearer {
            token: Some("{{token}}".to_string()),
            extra: BTreeMap::new(),
        },
        query: "query Search($q: String) { users(q: $q) { id } }".to_string(),
        // Invalid JSON on purpose: a half-typed variables editor must survive a save.
        variables: "{\"q\": ".to_string(),
        operation_name: Some("Search".to_string()),
        settings: Some(RequestSettings {
            timeout_ms: Some(5_000),
            ..RequestSettings::default()
        }),
        extra: BTreeMap::new(),
    });

    let decoded = round_trip(&request, "graphql");
    assert_eq!(decoded, request);
    let Request::GraphQl(graphql) = &decoded else {
        panic!("a graphql request must decode typed");
    };
    assert_eq!(graphql.variables, "{\"q\": ", "raw variables text survives");
}

#[test]
fn websocket_request_round_trips_with_saved_messages() {
    let request = Request::WebSocket(WebSocketRequest {
        id: ItemId::new(),
        name: "Chat".to_string(),
        url: "wss://echo.example.test/ws".to_string(),
        headers: vec![header("Origin", "https://example.test")],
        params: vec![header("room", "1")],
        subprotocols: vec!["graphql-transport-ws".to_string(), "chat".to_string()],
        auth: Auth::None,
        settings: None,
        saved_messages: vec![
            SavedMessage {
                name: "Hello".to_string(),
                kind: MessageKind::Text,
                payload: "hello".to_string(),
            },
            SavedMessage {
                name: "Join".to_string(),
                kind: MessageKind::Json,
                payload: "{\"join\":\"room-1\"}".to_string(),
            },
            SavedMessage {
                name: "Ping bytes".to_string(),
                kind: MessageKind::Binary,
                payload: "AAEC".to_string(),
            },
        ],
        extra: BTreeMap::new(),
    });

    let decoded = round_trip(&request, "websocket");
    assert_eq!(decoded, request);
    let Request::WebSocket(websocket) = &decoded else {
        panic!("a websocket request must decode typed");
    };
    let kinds: Vec<MessageKind> = websocket
        .saved_messages
        .iter()
        .map(|message| message.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![MessageKind::Text, MessageKind::Json, MessageKind::Binary]
    );
}

#[test]
fn socketio_request_defaults_path_and_namespace() {
    let input = r#"{
        "type": "socketio",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Live scores",
        "url": "https://scores.example.test"
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::SocketIo(socketio) = &request else {
        panic!("a socketio request must decode typed");
    };
    assert_eq!(socketio.path, "/socket.io");
    assert_eq!(socketio.namespace, "/");
    assert!(socketio.all_events, "all-events mode is on by default");
    assert!(socketio.listen.is_empty());
    assert!(socketio.saved_emits.is_empty());

    let full = Request::SocketIo(SocketIoRequest {
        path: "/ws".to_string(),
        namespace: "/admin".to_string(),
        headers: vec![header("X-Client", "pustman")],
        auth_payload: "{\"token\":\"{{token}}\"}".to_string(),
        handshake_timeout_ms: Some(3_000),
        listen: vec!["score".to_string()],
        all_events: false,
        saved_emits: vec![SavedEmit {
            name: "Subscribe".to_string(),
            event: "subscribe".to_string(),
            arguments: vec![SavedMessage {
                name: "match".to_string(),
                kind: MessageKind::Json,
                payload: "{\"match\":7}".to_string(),
            }],
            acknowledge: true,
        }],
        ..socketio.clone()
    });
    assert_eq!(round_trip(&full, "socketio"), full);
}

#[test]
fn grpc_request_round_trips_each_descriptor_source() {
    let sources = [
        DescriptorSource::Reflection,
        DescriptorSource::ProtoFiles {
            files: vec!["protos/helloworld.proto".to_string()],
            import_paths: vec!["protos".to_string()],
        },
        DescriptorSource::DescriptorSet {
            path: "protos/greeter.pb".to_string(),
        },
    ];

    for source in sources {
        let request = Request::Grpc(GrpcRequest {
            id: ItemId::new(),
            name: "Say hello".to_string(),
            target: "localhost:50051".to_string(),
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            message: "{\"name\":\"world\"}".to_string(),
            metadata: vec![header("x-request-id", "1")],
            auth: Auth::Inherit,
            deadline_ms: Some(2_000),
            tls: false,
            descriptor_source: source.clone(),
            extra: BTreeMap::new(),
        });

        let decoded = round_trip(&request, "grpc");
        assert_eq!(decoded, request, "{source:?} round-trips");
    }
}

#[test]
fn mqtt_kind_still_preserved_as_unsupported() {
    let input = r#"{
        "type": "mqtt",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Sensors",
        "url": "mqtt://broker.example.test:1883",
        "topic": "sensors/+/temperature"
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::Unsupported(raw) = &request else {
        panic!("an MQTT request is not modelled yet and must be kept raw");
    };
    assert_eq!(raw["topic"], "sensors/+/temperature");

    let output = serde_json::to_value(&request).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(output, original, "an MQTT request round-trips verbatim");
}
