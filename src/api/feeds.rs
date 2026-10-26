use std::sync::OnceLock;
use std::time::Duration;

use crate::model::Track;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("reqwest client")
    })
}

pub async fn latest(channel_id: &str) -> Result<Vec<Track>, reqwest::Error> {
    let xml = client()
        .get(format!(
            "https://www.youtube.com/feeds/videos.xml?channel_id={channel_id}"
        ))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse(&xml))
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(&xml[start..end])
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub fn parse(xml: &str) -> Vec<Track> {
    xml.split("<entry>")
        .skip(1)
        .filter_map(|entry| {
            let id = tag(entry, "yt:videoId")?.trim().to_string();
            if id.len() != 11 {
                return None;
            }
            Some(Track {
                id,
                title: tag(entry, "title").map(unescape).unwrap_or_default(),
                channel: tag(entry, "name").map(unescape).unwrap_or_default(),
                duration_secs: None,
                upload_day: tag(entry, "published").and_then(crate::model::parse_upload_day),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_feed_entries() {
        let xml = r#"<feed><title>Chan</title><author><name>Chan</name></author>
            <entry><id>yt:video:dQw4w9WgXcQ</id><yt:videoId>dQw4w9WgXcQ</yt:videoId>
            <title>Tom &amp; Jerry</title><author><name>Chan</name></author></entry>
            <entry><yt:videoId>9bZkp7q19f0</yt:videoId><title>Two</title>
            <author><name>Chan</name></author></entry></feed>"#;
        let t = parse(xml);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].title, "Tom & Jerry");
        assert_eq!(t[1].channel, "Chan");
    }
}
