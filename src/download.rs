use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::StreamExt;
use futures::TryStreamExt;
use indicatif::MultiProgress;
use indicatif::ProgressBar;
use indicatif::ProgressState;
use indicatif::ProgressStyle;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::metadata::Metadata;
use librespot::playback::config::PlayerConfig;
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::mixer::VolumeGetter;
use librespot::playback::player::Player;
use tokio::sync::Mutex;
use tokio::time::sleep;


use serde::{Serialize, Deserialize};

use crate::channel_sink::ChannelSink;
use crate::encoder::Format;
use crate::encoder::Samples;
use crate::channel_sink::SinkEvent;
use crate::track::Track;
use crate::DownloadState;


pub struct Downloader<'a> {
    player_config: PlayerConfig,
    session: &'a Session,
    progress_bar: MultiProgress,
    state: Arc<Mutex<DownloadState>>
}

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub destination: PathBuf,
    #[allow(dead_code)]
    pub compression: Option<u32>,
    pub parallel: usize,
    pub format: Format,
}

impl DownloadOptions {
    pub fn new(destination: Option<&str>, compression: Option<u32>, parallel: usize, format: Format) -> Self {
        let destination =
            destination.map_or_else(|| std::env::current_dir().unwrap(), PathBuf::from);
        DownloadOptions {
            destination,
            compression,
            parallel,
            format
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Action {
    Downloading { 
        file_name: String,
        downloaded_bytes: usize,
        total_bytes: usize 
    },
    Encoding { 
        file_name: String
    },
    Writing {
        file_name: String
    },
    Downloaded {
        file_name: String
    }
}

impl<'a> Downloader<'a> {
    pub fn new(session: &'a Session, state: Arc<Mutex<DownloadState>>) -> Self {
        Downloader {
            player_config: PlayerConfig::default(),
            session,
            progress_bar: MultiProgress::new(),
            state,
        }
    }

    pub async fn download_tracks(
        self,
        tracks: Vec<Track>,
        options: &DownloadOptions,
    ) -> Result<()> {
        futures::stream::iter(tracks)
            .map(|track| {
                self.download_track(track, options)
            })
            .buffer_unordered(options.parallel)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(())
    }

    #[tracing::instrument(name = "download_track", skip(self))]
    async fn download_track(&self, track: Track, options: &DownloadOptions) -> Result<()> {
        let metadata = track.metadata(&self.session).await?;
        tracing::info!("Downloading track: {:?}", metadata);

        let file_name = self.get_file_name(&track).await;
        let file_name_clone = file_name.clone();

        
        let mut path = String::new();


        if let Some(playlist) = track.playlist_id {

            let playlist_name = self.playlist_name(playlist).await?;

            path.push_str(options
                .destination
                .join(playlist_name)
                .join(file_name_clone.clone())
                .with_extension(options.format.extension())
                .to_str()
                .ok_or(anyhow::anyhow!("Could not set the output path"))?
                .to_string().as_str());
        } else if let Some(album) = track.album_id {
            let album_name = self.album_name(album).await?;

            path.push_str(options
                .destination
                .join(album_name)
                .join(file_name_clone.clone())
                .with_extension(options.format.extension())
                .to_str()
                .ok_or(anyhow::anyhow!("Could not set the output path"))?
                .to_string().as_str());
        } else {
            path.push_str(
                options
                .destination
                .join(file_name_clone.clone())
                .with_extension(options.format.extension())
                .to_str()
                .ok_or(anyhow::anyhow!("Could not set the output path"))?
                .to_string().as_str()
            );
        }

        let (sink, mut sink_channel) = ChannelSink::new(metadata);

        let file_size = sink.get_approximate_size();

        let (mut player, _) = Player::new(
            self.player_config.clone(),
            self.session.clone(),
            self.volume_getter(),
            move || Box::new(sink),
        );

        let pb = self.progress_bar.add(ProgressBar::new(file_size as u64));
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
            .with_key("eta", |state: &ProgressState, w: &mut dyn Write| write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap())
            .progress_chars("#>-"));

        let filename_clone_of_clone = file_name.clone();
        let message = Arc::new(Mutex::new(Action::Downloading {
            file_name: file_name.clone(),
            downloaded_bytes: 0,
            total_bytes: 0
        }));
        drop(filename_clone_of_clone);

       
        let stop_flag = Arc::new(Mutex::new(false));

        let message_clone = Arc::clone(&message);
        let stop_flag_clone = Arc::clone(&stop_flag);

        let sender = self.state.lock().await.sender.clone();

        tokio::spawn(async move {
            loop {
                let msg: Action;
                {
                    let guard = message_clone.lock().await;
                    msg = guard.clone();
                }

                let as_json_str = serde_json::to_string(&msg).unwrap();

                if let Err(e) = sender.send(crate::DownloadStateOpts::MessageSender(as_json_str)) {
                    tracing::error!("Error sending message via websocket: {:?}", e);
                }

                {
                    let stop_flag = stop_flag_clone.lock().await;
                    if *stop_flag {
                        break;
                    }
                }
                sleep(Duration::from_millis(500)).await;
            }
        });

        pb.set_message(file_name.clone());

        player.load(track.id, true, 0);

        let mut samples = Vec::<i32>::new();

        tokio::spawn(async move {
            player.await_end_of_track().await;
            player.stop();
        });

        while let Some(event) = sink_channel.recv().await {
    
            match event {
                SinkEvent::Write { bytes, total, mut content } => {
                    tracing::trace!("Written {} bytes out of {}", bytes, total);
                    pb.set_position(bytes as u64);
                    {
                        let mut msg = message.lock().await;
                        *msg = Action::Downloading {
                            file_name: file_name_clone.clone(),
                            downloaded_bytes: bytes,
                            total_bytes: total
                        };
                    }
                    samples.append(&mut content);
                }
                SinkEvent::Finished => {
                    tracing::info!("Finished downloading track: {:?}", file_name);
                    break;
                }
            }
            
        }

        tracing::info!("Encoding track: {:?}", &file_name_clone);
        pb.set_message(format!("Encoding {}", &file_name_clone));
        {
            let mut msg = message.lock().await;
            *msg = Action::Encoding { file_name: file_name_clone.clone() }
        }
        let samples = Samples::new(samples, 44100, 2, 16);
        let encoder = crate::encoder::get_encoder(options.format);
        let stream = encoder.encode(samples).await?;

        pb.set_message(format!("Writing {}", &file_name));
        {
            let mut msg = message.lock().await;
            *msg = Action::Writing { file_name: file_name_clone.clone() }
        }
        tracing::info!("Writing track: {:?} to file: {}", file_name, &path);
        stream.write_to_file(&path).await?;

        pb.finish_with_message(format!("Downloaded {}", &file_name));
        {
            let mut msg = message.lock().await;
            let mut stop_flag = stop_flag.lock().await;
            *msg = Action::Downloaded { file_name: file_name_clone.clone() };
            *stop_flag = true;
        }
        Ok(())
    }

    fn volume_getter(&self) -> Box<dyn VolumeGetter + Send> {
        Box::new(NoOpVolume)
    }

    async fn get_file_name(&self, track: &Track) -> String {

        let metadata = track.metadata(self.session).await.unwrap();
        let base62_id = track.id.to_base62().unwrap();

        // If there is more than 3 artists, add the first 3 and add "and others" at the end
        if metadata.artists.len() > 3 {
            let artists_name = metadata
                .artists
                .iter()
                .take(3)
                .map(|artist| artist.name.clone())
                .collect::<Vec<String>>()
                .join(", ");
            return self.clean_file_name(format!(
                "{}, and others - {} - {}",
                artists_name, metadata.track_name, base62_id
            ));
        }

        let artists_name = metadata
            .artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect::<Vec<String>>()
            .join(", ");
        self.clean_file_name(format!("{} - {} - {}", artists_name, metadata.track_name, base62_id))
    }

    fn clean_file_name(&self, file_name: String) -> String {
        let invalid_chars = ['<', '>', ':', '\'', '"', '/', '\\', '|', '?', '*'];
        let mut clean = String::new();

        // Everything but Windows should allow non-ascii characters
        let allows_non_ascii = !cfg!(windows);
        for c in file_name.chars() {
            if !invalid_chars.contains(&c) && (c.is_ascii() || allows_non_ascii) && !c.is_control()
            {
                clean.push(c);
            }
        }
        clean
    }

    async fn playlist_name(&self, id: SpotifyId) -> Result<String> {
        let playlist = librespot::metadata::Playlist::get(self.session, id)
            .await.unwrap();

        let playlist_name = playlist.name;
        let playlist_creator = playlist.user;
        let base62_id = id.to_base62().unwrap();

        Ok(self.clean_file_name(format!("{} - {} - {}", playlist_creator, playlist_name, base62_id)))
            
    }

    async fn album_name(&self, id: SpotifyId) -> Result<String> {
        let album: librespot::metadata::Album = librespot::metadata::Album::get(self.session, id)
            .await.unwrap();

        let album_name: String = album.name;
        let album_artist_vec: Vec<SpotifyId> = album.artists;
        let base62_id = id.to_base62().unwrap();

        let mut artists_string: String = String::new();

        for (i, artist_id) in album_artist_vec.iter().enumerate() {
            if i >= 3 {
                artists_string.push_str("and others...");
                break;
            }
            let artist_str = self.convert_artist_to_string(artist_id.clone(), self.session).await.unwrap();
            artists_string.push_str(&format!("{}, ", artist_str));
        }

        if artists_string.ends_with(", ") {
            artists_string.truncate(artists_string.len() - 2);
        }

        Ok(self.clean_file_name(format!("{} - {} - {}", artists_string, album_name, base62_id)))
        
    }

    async fn convert_artist_to_string(&self, id: SpotifyId, session: &Session) -> Result<String> {
        let artist = librespot::metadata::Artist::get(session, id)
            .await.unwrap();

        Ok(artist.name)
    }
}
