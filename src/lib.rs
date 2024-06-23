use std::{fs, path::PathBuf, sync::Arc};
use anyhow::Result;
use download::DownloadOptions;
use futures::SinkExt;
use librespot::core::session::Session;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;


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
        ws_url: Option<String>
    ) -> Result<Self> {

        let output_folder = destination_folder(output_folder_name)?;

        let session = create_session(&username, &password).await?;

        let (sender, mut receiver) = broadcast::channel(10);
        
        let state = Arc::new(Mutex::new(DownloadState { sender }));

        if let Some(url) = ws_url {
            let url = Url::parse(&url)?;

            tokio::task::spawn(async move {
                let (mut ws_stream, _) = connect_async(url).await.unwrap();

                while let Ok(msg) = receiver.recv().await {
                    match msg {
                        DownloadStateOpts::MessageSender(msg) => {
                            ws_stream.send(Message::Text(msg.into())).await.unwrap();
                        }
                        _ => {}
                    }
                }
            });

        }

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

        let state = &self.state.lock().await;

        state.sender.send(DownloadStateOpts::SocketCloser)?;

        Ok(())
    }
}


pub async fn verify_login( username: &str, password: &str) -> Result<()> {
    let _session = create_session(&username, &password).await?;
    Ok(())
}

