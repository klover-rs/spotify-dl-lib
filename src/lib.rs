use std::fs;
use anyhow::Result;
use download::DownloadOptions;
use librespot::core::session::Session;

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

fn create_output_folder(folder_name: &str) -> Result<String> {
    if let Some(home_dir) = dirs::home_dir() {
        let folder_path = home_dir.join(folder_name);

        if !folder_path.exists() {
            fs::create_dir(&folder_path)?;
        }

        let data_folder = folder_path.to_string_lossy().into_owned();
        Ok(data_folder)
    } else {
        anyhow::bail!("Home directory not found");
    }
}


pub struct SpotifyDownloader {
    output_folder: String,
    session: Session
    
}

impl SpotifyDownloader {
    pub async fn new(output_folder_name: &str, username: &str, password: &str) -> Result<Self> {

        let output_folder = create_output_folder(&output_folder_name)?;

        let session = create_session(&username, &password).await?;

        Ok(Self {
            output_folder: output_folder,
            session: session
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

        let downloader = Downloader::new(&self.session);
        downloader.download_tracks(
            tracks,
            &DownloadOptions::new(Some(&self.output_folder), Some(compression), parallel, format),
        ).await
    }
}





