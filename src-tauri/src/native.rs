//! native messaging host. arayüz açmaz, uygulama kapalıysa tepside başlatıp mesajı yerel köprüye iletir.
//! chrome `chrome-extension://<id>/`, firefox `<manifest> <id>` argümanıyla çağırıyor.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;

pub const HOST: &str = "com.trawl.bridge";
/// paketlenmemiş eklentinin sabit kimliği (manifest "key"), mağaza kimlikleri çıkınca buraya
pub const CHROME_IDS: &[&str] = &["elnkcfkeoeihikkacnjfadeppelgghjl"];
pub const GECKO_ID: &str = "trawl@trawl.app";
const PATHS: &[&str] = &["/ping", "/add", "/grab", "/jobs", "/refresh-result", "/probe-stream", "/site", "/show"];

/// native host olarak açıldıysak köprüye gidecek Origin
pub fn host_origin() -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(o) = args.iter().find(|a| a.starts_with("chrome-extension://")) {
        let id = o.trim_start_matches("chrome-extension://").trim_end_matches('/');
        return CHROME_IDS.contains(&id).then(|| format!("chrome-extension://{id}"));
    }
    args.iter().any(|a| a == GECKO_ID).then(|| "moz-extension://trawl".to_string())
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().expect("http")
}

async fn running(c: &reqwest::Client) -> bool {
    let u = format!("http://127.0.0.1:{}/ping", crate::bridge::PORT);
    matches!(c.get(u).timeout(Duration::from_millis(800)).send().await, Ok(r) if r.status().is_success())
}

fn spawn_app() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    const DETACHED: u32 = 0x0000_0008;
    const NEW_GROUP: u32 = 0x0000_0200;
    const BREAKAWAY: u32 = 0x0100_0000; // tarayıcının job nesnesinden çık, host kapanınca uygulama da ölmesin
    let go = |flags: u32| {
        std::process::Command::new(&exe)
            .arg("--tray")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
            .is_ok()
    };
    go(DETACHED | NEW_GROUP | BREAKAWAY) || go(DETACHED | NEW_GROUP)
}

pub async fn handle(origin: &str, msg: &Value) -> Value {
    let path = msg["path"].as_str().unwrap_or("/ping");
    if !PATHS.contains(&path) {
        return json!({"ok": false, "error": "bilinmeyen yol"});
    }
    let c = http();
    if !running(&c).await {
        if !msg["launch"].as_bool().unwrap_or(false) || !spawn_app() {
            return json!({"ok": false, "running": false});
        }
        let mut up = false;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if running(&c).await {
                up = true;
                break;
            }
        }
        if !up {
            return json!({"ok": false, "running": false, "error": "Trawl açılamadı"});
        }
    }
    let u = format!("http://127.0.0.1:{}{path}", crate::bridge::PORT);
    let rb = if path == "/ping" { c.get(u) } else { c.post(u).json(&msg["body"]) };
    match rb.header("Origin", origin).send().await {
        Ok(r) => {
            let st = r.status().as_u16();
            let data: Value = r.json().await.unwrap_or(Value::Null);
            json!({"ok": (200..300).contains(&st), "running": true, "status": st, "data": data})
        }
        Err(e) => json!({"ok": false, "running": true, "error": e.to_string()}),
    }
}

/// 4 bayt LE uzunluk + JSON, tarayıcı kanalı kapatınca biter
pub fn run_host(origin: String) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio");
    let mut inp = std::io::stdin().lock();
    let mut out = std::io::stdout().lock();
    loop {
        let mut len = [0u8; 4];
        if inp.read_exact(&mut len).is_err() {
            break;
        }
        let n = u32::from_le_bytes(len) as usize;
        if n == 0 || n > 4 << 20 {
            break;
        }
        let mut buf = vec![0u8; n];
        if inp.read_exact(&mut buf).is_err() {
            break;
        }
        let reply = match serde_json::from_slice::<Value>(&buf) {
            Ok(m) => rt.block_on(handle(&origin, &m)),
            Err(_) => json!({"ok": false, "error": "geçersiz mesaj"}),
        };
        let b = serde_json::to_vec(&reply).unwrap_or_default();
        if out.write_all(&(b.len() as u32).to_le_bytes()).and_then(|_| out.write_all(&b)).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn reg_default(subkey: &str, value: &str) -> bool {
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let (k, v) = (wide(subkey), wide(value));
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), std::ptr::null(), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) == 0 }
}

// her açılışta manifestleri yaz, kayıtları bu exe'ye bağla ki exe taşınsa da yol doğru kalsın. HKCU, yönetici gerekmez
pub fn register() {
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.to_string_lossy().to_string();
    let dir = crate::util::data_dir();
    let base = json!({"name": HOST, "description": "Trawl indirme yöneticisi köprüsü", "path": exe, "type": "stdio"});
    let mut chrome = base.clone();
    chrome["allowed_origins"] = json!(CHROME_IDS.iter().map(|i| format!("chrome-extension://{i}/")).collect::<Vec<_>>());
    let mut ff = base;
    ff["allowed_extensions"] = json!([GECKO_ID]);
    let (cp, fp) = (dir.join("native-chromium.json"), dir.join("native-firefox.json"));
    let ok = std::fs::write(&cp, serde_json::to_vec_pretty(&chrome).unwrap()).is_ok() && std::fs::write(&fp, serde_json::to_vec_pretty(&ff).unwrap()).is_ok();
    if !ok {
        return;
    }
    let (cp, fp) = (cp.to_string_lossy().to_string(), fp.to_string_lossy().to_string());
    for vendor in [r"Google\Chrome", "Chromium", r"Microsoft\Edge", r"BraveSoftware\Brave-Browser", "Vivaldi"] {
        reg_default(&format!(r"Software\{vendor}\NativeMessagingHosts\{HOST}"), &cp);
    }
    reg_default(&format!(r"Software\Mozilla\NativeMessagingHosts\{HOST}"), &fp);
}

#[cfg(test)]
mod tests {
    #[test]
    fn registry_default_value_roundtrip() {
        let key = r"Software\TrawlTest\NativeMessagingHosts\com.trawl.bridge";
        assert!(super::reg_default(key, r"C:\Yol\native-chromium.json"));
        let out = std::process::Command::new("reg").args(["query", &format!(r"HKCU\{key}"), "/ve"]).output().unwrap();
        let _ = std::process::Command::new("reg").args(["delete", r"HKCU\Software\TrawlTest", "/f"]).output();
        assert!(String::from_utf8_lossy(&out.stdout).contains(r"C:\Yol\native-chromium.json"));
    }

    #[test]
    fn rejects_unknown_paths_without_network() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let r = rt.block_on(super::handle("chrome-extension://x", &serde_json::json!({"path": "/../etc"})));
        assert_eq!(r["ok"], false);
    }
}
