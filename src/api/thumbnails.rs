//! Thumbnail download. GPUI's own HTTP client is a null client, so bytes come from `reqwest`.

use std::sync::OnceLock;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("reqwest client")
    })
}

pub fn url(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg")
}

/// The raw JPEG bytes of the medium-quality thumbnail.
pub async fn fetch(video_id: &str) -> Result<Vec<u8>, reqwest::Error> {
    let bytes = client()
        .get(url(video_id))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}
