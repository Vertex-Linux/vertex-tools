use anyhow::Result;
use std::io::Read;

pub fn fetch_json(url: &str) -> Result<serde_json::Value> {
    let body = ureq::get(url)
        .set("User-Agent", crate::constants::USER_AGENT)
        .call()?
        .into_string()?;
    Ok(serde_json::from_str(&body)?)
}

pub fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    ureq::get(url)
        .set("User-Agent", crate::constants::USER_AGENT)
        .call()?
        .into_reader()
        .read_to_end(&mut buf)?;
    Ok(buf)
}
