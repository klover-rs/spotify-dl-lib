use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use anyhow::Result;
use axum::{extract::{ws::{Message, WebSocket}, WebSocketUpgrade}, response::IntoResponse, routing::get, Router};
use download::DownloadOptions;
use librespot::core::session::Session;
use tokio::sync::{broadcast, Mutex};


mod session;
mod channel_sink;
mod track;
mod encoder;
mod download;

use crate::{
    session::create_session,
    track::get_tracks,
    download::Downloader,
    encoder::Format
};

lazy_static::lazy_static! {
    static ref STOP_FLAG: Mutex<bool> = Mutex::new(false);
}

fn destination_folder(folder_path: PathBuf) -> Result<String> {
    if !folder_path.exists() {
        fs::create_dir(&folder_path)?;
    }

    let folder_str = folder_path.to_string_lossy().into_owned();

    Ok(folder_str)
}

#[derive(Debug, Clone)]
pub enum DownloadStateOpts {
    MessageSender(String),
    SocketCloser,
    ShutdownSignal
}

struct DownloadState {
    sender: broadcast::Sender<DownloadStateOpts>
}

async fn websocket_handler(ws: WebSocketUpgrade, state: Arc<Mutex<DownloadState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<Mutex<DownloadState>>) {
    let mut receiver = state.lock().await.sender.subscribe();

    loop {
        tokio::select! {
            Some(msg) = socket.recv() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        println!("got message: {}", text);
                    } 
                    Ok(Message::Close(_)) => {
                        println!("client disconnected");
                        break;
                    }
                    Ok(_) => {},
                    Err(e) => {
                        println!("websocket e: {}", e);
                    }
                }
            }
            Ok(message) = receiver.recv() => {
                match message {
                    DownloadStateOpts::MessageSender(msg) => {
                        if socket.send(Message::Text(msg)).await.is_err() {
                            println!("client disconnected");
                            break;
                        }
                    }
                    DownloadStateOpts::SocketCloser => {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                    _ => {}
                }
                
               
            }
        }
    }
}

pub struct SpotifyDownloader {
    output_folder: String,
    session: Session,
    state: Arc<Mutex<DownloadState>>,

}

impl SpotifyDownloader {
    pub async fn new(
        output_folder_name: PathBuf, 
        username: &str, 
        password: &str,
    ) -> Result<Self> {

        let output_folder = destination_folder(output_folder_name)?;

        let session = create_session(&username, &password).await?;

        let (sender, _receiver) = broadcast::channel(10);
        
        let state = Arc::new(Mutex::new(DownloadState { sender }));

        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            let app = Router::new()
                .route("/send", get({
                    move |ws| websocket_handler(ws, state_clone)
                }));

            let listener = tokio::net::TcpListener::bind("127.0.0.1:4040").await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap(); 

            println!("L");
        });

        Ok(Self {
            output_folder,
            session,
            state,

        })
    }

    pub async fn download_tracks(
        &self,
        track_url: Vec<String>,
        parallel: Option<usize>,
        compression: Option<u32>,
        format: &str,
    ) -> Result<()> {

        let format = match format {
            "mp3" => Format::Mp3,
            "flac" => Format::Flac,
            _ => panic!("unsupported format provided")
        };

        let parallel = parallel.unwrap_or(5);
        let compression = compression.unwrap_or(4);

        let tracks = get_tracks(track_url, &self.session).await?;

        let downloader = Downloader::new(&self.session, Arc::clone(&self.state));
        downloader.download_tracks(
            tracks,
            &DownloadOptions::new(Some(&self.output_folder), Some(compression), parallel, format),
        ).await?;

        println!("all tracks were downloaded!");

        let mut stop_flag = STOP_FLAG.lock().await;
        *stop_flag = true;

        Ok(())
    }
}


pub async fn verify_login( username: &str, password: &str) -> Result<()> {
    let _session = create_session(&username, &password).await?;
    Ok(())
}

async fn shutdown_signal() {
    loop {
        {
            let mut stop_flag = STOP_FLAG.lock().await;
            if *stop_flag {
                *stop_flag = false;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("Shutdown signal received, initiating shutdown.");
}