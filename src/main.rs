use std::error::Error;
use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Result, middleware};
use actix_ws;
use bluest::{btuuid::bluetooth_uuid_from_u16, Adapter, Device, Uuid};
use futures_lite::stream::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};
use tokio::time::{sleep, Duration};
use actix_web::rt::spawn;

// Define heart rate data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartRateData {
    pub value: u16,
    pub sensor_contact_detected: Option<bool>,
    pub timestamp: u64,
}

// Global state management
#[derive(Clone)]
pub struct AppState {
    pub heart_rate_data: Arc<RwLock<Option<HeartRateData>>>,
    pub tx: broadcast::Sender<HeartRateData>,
}

const HRS_UUID: Uuid = bluetooth_uuid_from_u16(0x180D);
const HRM_UUID: Uuid = bluetooth_uuid_from_u16(0x2A37);

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    // Create broadcast channel for WebSocket push
    let (tx, _) = broadcast::channel::<HeartRateData>(100);

    // Create global state
    let app_state = AppState {
        heart_rate_data: Arc::new(RwLock::new(None)),
        tx,
    };

    // Start Bluetooth monitoring task
    let state_clone = app_state.clone();
    spawn(async move {
        if let Err(e) = start_bluetooth_monitor(state_clone).await {
            log::error!("Bluetooth monitoring failed: {:?}", e);
        }
    });

    // Start Web server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .wrap(cors_config())
            .wrap(middleware::Logger::default())
            .route("/api/heart-rate", web::get().to(get_heart_rate))
            .route("/api/ws", web::get().to(ws_handler))
            .service(actix_files::Files::new("/", "./static").index_file("index.html"))
    })
    .bind("0.0.0.0:28040")?
    .run()
    .await?;

    Ok(())
}

fn cors_config() -> Cors {
    Cors::default()
        .allow_any_origin()
        .allow_any_method()
        .allow_any_header()
        .supports_credentials()
}

// API endpoint to get current heart rate data
async fn get_heart_rate(data: web::Data<AppState>) -> Result<HttpResponse> {
    let heart_rate = data.heart_rate_data.read().await.clone();
    
    match heart_rate {
        Some(hr_data) => {
            Ok(HttpResponse::Ok()
                .content_type("application/json")
                .json(hr_data))
        },
        None => {
            Ok(HttpResponse::NotFound()
                .content_type("application/json")
                .json(serde_json::json!({"error": "No heart rate data available"})))
        }
    }
}

// WebSocket处理器
async fn ws_handler(
    req: actix_web::HttpRequest,
    stream: actix_web::web::Payload,
    data: web::Data<AppState>,
) -> Result<actix_web::HttpResponse> {
    let (response, mut session, msg_stream) = actix_ws::handle(&req, stream)?;

    // Subscribe to broadcast channel
    let mut rx = data.tx.subscribe();

    // Send current heart rate data to newly connected clients
    {
        let current_data = data.heart_rate_data.read().await.clone();
        if let Some(hr_data) = current_data {
            let _ = session.text(serde_json::to_string(&hr_data).unwrap()).await;
        }
    }

    // Create a flag to control task lifecycle
    let session_closed = Arc::new(tokio::sync::Notify::new());
    let session_closed_clone = session_closed.clone();

    // Start message handling task
    let mut session_for_msg = session.clone();
    actix_web::rt::spawn(async move {
        let mut msg_stream = msg_stream;
        
        while let Some(msg) = msg_stream.next().await {
            match msg {
                Ok(actix_ws::Message::Ping(bytes)) => {
                    if session_for_msg.pong(&bytes).await.is_err() {
                        break;
                    }
                }
                Ok(actix_ws::Message::Pong(_)) => {}
                Ok(actix_ws::Message::Text(text)) => {
                    // Respond to simple ping message
                    if text == "ping" {
                        if session_for_msg.text("pong").await.is_err() {
                            break;
                        }
                    }
                }
                Ok(actix_ws::Message::Close(_reason)) => {
                    break;
                }
                Err(_) | Ok(_) => break,
            }
        }
        session_closed_clone.notify_waiters(); // Notify other tasks that connection is closed
    });

    // Start message forwarding task
    let mut session_for_broadcasts = session;
    let closed_notifier = session_closed.clone();
    actix_web::rt::spawn(async move {
        loop {
            tokio::select! {
                Ok(heart_rate_data) = rx.recv() => {
                    if session_for_broadcasts.text(serde_json::to_string(&heart_rate_data).unwrap()).await.is_err() {
                        // Exit loop if sending fails
                        break;
                    }
                }
                _ = closed_notifier.notified() => {
                    // Exit when connection is closed
                    break;
                }
            }
        }
    });

    Ok(response)
}

// Start Bluetooth monitoring task
async fn start_bluetooth_monitor(app_state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let adapter = Adapter::default()
        .await
        .ok_or("Bluetooth adapter not found")?;
    adapter.wait_available().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    loop {
        let device = {
            let connected_heart_rate_devices =
                adapter.connected_devices_with_services(&[HRS_UUID]).await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            if let Some(device) = connected_heart_rate_devices.into_iter().next() {
                device
            } else {
                println!("Ready to scan");
                let mut scan = adapter.discover_devices(&[HRS_UUID]).await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

                println!("Scanning devices");
                let device = scan.next().await
                    .ok_or("No device found")?
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

                println!("Found Device: [{}] {:?}", device, device.name_async().await);
                device
            }
        };

        if let Err(err) = handle_device(&adapter, &device, &app_state).await {
            println!("Connection error: {err:?}");
            // Wait for a period before retrying
            sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn handle_device(adapter: &Adapter, device: &Device, app_state: &AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Connect
    if !device.is_connected().await {
        println!("Connecting device: {}", device.id());
        adapter.connect_device(&device).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    }

    // Discover services
    let heart_rate_services = device.discover_services_with_uuid(HRS_UUID).await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    let heart_rate_service = heart_rate_services
        .first()
        .ok_or("Device should has one heart rate service at least")?;

    // Discover
    let heart_rate_measurements = heart_rate_service
        .discover_characteristics_with_uuid(HRM_UUID)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    let heart_rate_measurement = heart_rate_measurements
        .first()
        .ok_or("HeartRateService should has one heart rate measurement characteristic at least")?;

    let mut updates = heart_rate_measurement.notify().await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    while let Some(Ok(heart_rate)) = updates.next().await {
        let flag = *heart_rate.get(0).ok_or("No flag")?;

        // Heart Rate Value Format
        let mut heart_rate_value = *heart_rate.get(1).ok_or("No heart rate u8")? as u16;
        if flag & 0b00001 != 0 {
            heart_rate_value |= (*heart_rate.get(2).ok_or("No heart rate u16")? as u16) << 8;
        }

        // Sensor Contact Supported
        let mut sensor_contact = None;
        if flag & 0b00100 != 0 {
            sensor_contact = Some(flag & 0b00010 != 0)
        }
        
        let heart_rate_data = HeartRateData {
            value: heart_rate_value,
            sensor_contact_detected: sensor_contact,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        println!("HeartRateValue: {heart_rate_value}, SensorContactDetected: {sensor_contact:?}");

        // Update global state
        {
            let mut state = app_state.heart_rate_data.write().await;
            *state = Some(heart_rate_data.clone());
        }

        // Send to all WebSocket clients via broadcast channel
        let _ = app_state.tx.send(heart_rate_data);
    }
    
    Ok(())
}