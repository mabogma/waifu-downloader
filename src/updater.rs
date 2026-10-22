use crate::install;
use reqwest::blocking::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};
const API: &str = "https://api.github.com/repos/mabogma/waifu-downloader/releases/latest";
const PREFIX: &str = "https://github.com/mabogma/waifu-downloader/releases/download/";
const LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Clone)]
pub struct Ready {
    pub version: String,
    pub path: PathBuf,
    pub digest: String,
}
pub enum Event {
    Current,
    Ready(Ready),
    Error(String),
}
pub struct Updater {
    pub tx: SyncSender<PathBuf>,
    pub rx: Receiver<Event>,
}
fn version(s: &str) -> Option<[u64; 3]> {
    let v: Vec<_> = s
        .strip_prefix('v')
        .unwrap_or(s)
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<_, _>>()
        .ok()?;
    v.try_into().ok()
}
fn release(v: &Value, current: &str) -> Result<Option<(String, String, String, u64)>, String> {
    if v["draft"].as_bool().unwrap_or(false) || v["prerelease"].as_bool().unwrap_or(false) {
        return Ok(None);
    }
    let tag = v["tag_name"].as_str().ok_or("В релизе нет номера версии")?;
    let newest = version(tag).ok_or("Тег релиза должен иметь вид v2.2.1")?;
    if newest <= version(current).ok_or("Неизвестная версия приложения")?
    {
        return Ok(None);
    }
    let asset = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"] == "WaifuGallery.exe"))
        .ok_or("В новом релизе нет файла WaifuGallery.exe")?;
    let url = asset["browser_download_url"]
        .as_str()
        .filter(|u| u.starts_with(PREFIX))
        .ok_or("Неверный адрес обновления")?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("GitHub не предоставил SHA-256 обновления")?;
    let size = asset["size"]
        .as_u64()
        .filter(|s| *s > 0 && *s <= LIMIT)
        .ok_or("Неподдерживаемый размер обновления")?;
    Ok(Some((
        format!("{}.{}.{}", newest[0], newest[1], newest[2]),
        url.into(),
        digest.to_ascii_lowercase(),
        size,
    )))
}
fn bounded(response: reqwest::blocking::Response, max: u64) -> Result<Vec<u8>, String> {
    let mut data = vec![];
    response
        .take(max + 1)
        .read_to_end(&mut data)
        .map_err(|_| "Загрузка обновления прервана".to_string())?;
    if data.len() as u64 > max {
        return Err("Обновление превышает допустимый размер".into());
    }
    Ok(data)
}
fn verify(bytes: &[u8], digest: &str) -> Result<(), String> {
    if format!("{:x}", Sha256::digest(bytes)) != digest {
        return Err("Контрольная сумма обновления не совпала".into());
    }
    let offset = bytes
        .get(0x3c..0x40)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        .ok_or("Неверный EXE")?;
    if bytes.get(..2) != Some(b"MZ")
        || bytes.get(offset..offset.saturating_add(6)) != Some(b"PE\0\0\x64\x86")
    {
        return Err("Обновление не является Windows x64 EXE".into());
    }
    Ok(())
}
fn check(client: &Client, base: &Path) -> Result<Event, String> {
    let response = client
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| "Не удалось проверить обновления".to_string())?;
    if response.status().as_u16() == 404 {
        return Ok(Event::Current);
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub: ошибка {}. Попробуй позже",
            response.status().as_u16()
        ));
    }
    let data: Value = serde_json::from_slice(&bounded(response, 2 * 1024 * 1024)?)
        .map_err(|_| "Неверный ответ GitHub".to_string())?;
    let Some((version, url, digest, size)) = release(&data, env!("CARGO_PKG_VERSION"))? else {
        return Ok(Event::Current);
    };
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| "Не удалось скачать обновление".to_string())?;
    let bytes = bounded(response, LIMIT)?;
    if bytes.len() as u64 != size {
        return Err("Обновление скачано не полностью".into());
    }
    verify(&bytes, &digest)?;
    let dir = base.join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("WaifuGallery-{version}.exe"));
    let temp = path.with_extension("download");
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &path).map_err(|e| e.to_string())?;
    Ok(Event::Ready(Ready {
        version,
        path,
        digest,
    }))
}
impl Updater {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (tx, jobs) = mpsc::sync_channel::<PathBuf>(1);
        let (events, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let client = Client::builder()
                .timeout(Duration::from_secs(120))
                .connect_timeout(Duration::from_secs(10))
                .user_agent(concat!("WaifuGallery/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() > 5 {
                        return attempt.error("too many redirects");
                    }
                    if attempt.url().scheme() == "https"
                        && matches!(
                            attempt.url().host_str(),
                            Some(
                                "github.com"
                                    | "release-assets.githubusercontent.com"
                                    | "objects.githubusercontent.com"
                            )
                        )
                    {
                        attempt.follow()
                    } else {
                        attempt.error("untrusted update redirect")
                    }
                }))
                .build();
            while let Ok(base) = jobs.recv() {
                let event = match client.as_ref() {
                    Ok(c) => check(c, &base).unwrap_or_else(Event::Error),
                    Err(_) => Event::Error("Сетевой клиент обновлений недоступен".into()),
                };
                if events.send(event).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { tx, rx }
    }
}
pub fn schedule(ready: &Ready, base: &Path, restart: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script = std::env::temp_dir().join(format!("waifu-update-{}.ps1", std::process::id()));
    std::fs::write(&script, include_str!("../windows/update.ps1")).map_err(|e| e.to_string())?;
    let mut cmd = install::powershell(&script);
    cmd.arg("-OldPid")
        .arg(std::process::id().to_string())
        .arg("-Target")
        .arg(exe)
        .arg("-Staged")
        .arg(&ready.path)
        .arg("-ExpectedDigest")
        .arg(&ready.digest)
        .arg("-DataDir")
        .arg(base);
    if restart {
        cmd.arg("-Restart");
    };
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_versions_and_release_validation() {
        assert!(version("v2.10.0") > version("v2.9.0"));
        assert!(version("v2.3.0-beta").is_none());
        let mut r = serde_json::json!({"tag_name":"v9.0.0","assets":[{"name":"WaifuGallery.exe","browser_download_url":format!("{PREFIX}v9.0.0/WaifuGallery.exe"),"size":1024,"digest":format!("sha256:{}","a".repeat(64))}]});
        assert!(release(&r, "2.2.0").unwrap().is_some());
        assert!(release(&r, "9.0.0").unwrap().is_none());
        r["assets"][0]["browser_download_url"] = "https://example.com/malware.exe".into();
        assert!(release(&r, "2.2.0").is_err());
    }
    #[test]
    fn checksum_and_architecture_are_required() {
        let mut b = vec![0u8; 256];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        b[128..134].copy_from_slice(b"PE\0\0\x64\x86");
        assert!(verify(&b, &format!("{:x}", Sha256::digest(&b))).is_ok());
        assert!(verify(&b, &"0".repeat(64)).is_err());
        b[133] = 0;
        assert!(verify(&b, &format!("{:x}", Sha256::digest(&b))).is_err());
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
