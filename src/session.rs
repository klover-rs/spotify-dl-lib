use anyhow::Result;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::discovery::Credentials;

pub async fn create_session(username: &str, password: &str) -> Result<Session> {
    let session_config = SessionConfig::default();
    let credentials = Credentials::with_password(username, password);

    let (session, _) = Session::connect(session_config, credentials, None, false).await?;

    Ok(session)
    
} 
