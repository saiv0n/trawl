//! süresi dolan imzalı bağlantıları kendiliğinden tazeleme.
//! eklenti kaynak sayfayı arka sekmede açıp adayları yollar, boyutu birebir tutan ilk aday aynı dosya sayılır.

use crate::manager::Manager;
use crate::model::{now, Kind, Status, Task};
use crate::util::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub const STAGE: &str = "Bağlantı yenileniyor";
const MAX_TRIES: u32 = 3;
const WAIT_SECS: u64 = 100;

#[derive(Serialize, Clone, Debug)]
pub struct Job {
    pub id: String,
    pub page: String,
    pub url: String,
    pub name: String,
    pub size: u64,
    #[serde(skip)]
    pub err: String,
    #[serde(skip)]
    pub taken: bool,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct Cand {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// süresi dolmuş bağlantı gibi duran hata mesajı mı
pub fn expired(msg: &str) -> bool {
    msg.contains("süresi dolmuş") || ["401", "403", "404", "410"].iter().any(|c| msg.contains(&format!("Sunucu {c} döndü")))
}

fn tail(u: &str) -> String {
    url::Url::parse(u)
        .ok()
        .and_then(|x| x.path_segments().and_then(|mut s| s.next_back().map(|t| t.to_lowercase())))
        .unwrap_or_default()
}

/// dosya adı tutanlar önce, sonra aynı host; tekrarlar atılır
pub fn rank(cands: Vec<Cand>, old_url: &str, name: &str) -> Vec<Cand> {
    let (ot, nm, oh) = (tail(old_url), name.to_lowercase(), host_of(old_url));
    let mut seen = std::collections::HashSet::new();
    let mut v: Vec<(u8, Cand)> = cands
        .into_iter()
        .filter(|c| c.url.starts_with("http") && seen.insert(c.url.clone()))
        .map(|c| {
            let t = tail(&c.url);
            let s = if !t.is_empty() && (t == ot || t == nm) { 0 } else if host_of(&c.url) == oh { 1 } else { 2 };
            (s, c)
        })
        .collect();
    v.sort_by_key(|(s, _)| *s);
    v.into_iter().map(|(_, c)| c).take(15).collect()
}

/// start()'ın hata dalından çağrılıyor; true dönerse görev hataya düşmez
pub fn try_start(m: &Arc<Manager>, t: &Arc<Task>, msg: &str) -> bool {
    if !expired(msg) || now() - m.ext_seen.load(Ordering::Relaxed) > 90 {
        return false;
    }
    let job = {
        let mut d = t.d.lock();
        if d.kind != Kind::Http || d.page.is_empty() || d.size == 0 || d.headers.contains_key(BODY_KEY) || d.refresh_tries >= MAX_TRIES {
            return false;
        }
        d.refresh_tries += 1;
        d.status = Status::Probing;
        d.stage = STAGE.into();
        d.error.clear();
        Job { id: d.id.clone(), page: d.page.clone(), url: d.url.clone(), name: d.name.clone(), size: d.size, err: msg.to_string(), taken: false }
    };
    let id = job.id.clone();
    {
        let mut q = m.refresh.lock();
        q.retain(|j| j.id != id);
        q.push(job);
    }
    // eklenti cevap vermezse sonsuza dek beklemesin
    let (m2, t2) = (m.clone(), t.clone());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(WAIT_SECS)).await;
        let job = {
            let mut q = m2.refresh.lock();
            let i = q.iter().position(|j| j.id == id);
            i.map(|i| q.remove(i))
        };
        if let Some(j) = job {
            fail(&m2, &t2, &j.err, "Eklenti sayfayı yeniden açamadı.");
        }
    });
    true
}

fn waiting(t: &Task) -> bool {
    let d = t.d.lock();
    d.status == Status::Probing && d.stage == STAGE
}

fn fail(m: &Manager, t: &Task, err: &str, why: &str) {
    if !waiting(t) {
        return;
    }
    let (id, name, msg) = {
        let mut d = t.d.lock();
        d.status = Status::Error;
        d.stage.clear();
        d.error = format!("{} {why} Sayfadan yeni bağlantıyı kopyalayıp \"Bağlantıyı güncelle\" ile sürdürebilirsin.", clean_err(err));
        (d.id.clone(), d.name.clone(), d.error.clone())
    };
    m.notice(serde_json::json!({"type": "error", "id": id, "name": name, "error": msg, "tools": false}));
}

/// her iş bir kez verilir
pub fn take(m: &Manager) -> Vec<Job> {
    m.ext_seen.store(now(), Ordering::Relaxed);
    let mut q = m.refresh.lock();
    q.iter_mut()
        .filter(|j| !j.taken)
        .map(|j| {
            j.taken = true;
            j.clone()
        })
        .collect()
}

/// boyutu birebir tutan ilk adayı (son url + birleşik başlıklar) döndürür
pub async fn pick(cands: Vec<Cand>, job: &Job, headers: &BTreeMap<String, String>) -> Option<(String, BTreeMap<String, String>)> {
    for c in rank(cands, &job.url, &job.name) {
        let mut h = headers.clone();
        for (k, v) in c.headers {
            if !hop_header(&k) && !v.is_empty() {
                hput(&mut h, &k, v);
            }
        }
        let Ok(Ok(p)) = tokio::time::timeout(Duration::from_secs(15), crate::http::probe(&c.url, &h)).await else { continue };
        if p.size == job.size {
            return Some((p.url, h));
        }
    }
    None
}

pub async fn result(m: Arc<Manager>, id: String, cands: Vec<Cand>) -> bool {
    let job = {
        let mut q = m.refresh.lock();
        let Some(i) = q.iter().position(|j| j.id == id) else { return false };
        q.remove(i)
    };
    let Some(t) = m.find(&id) else { return false };
    if !waiting(&t) {
        return false;
    }
    let headers = t.d.lock().headers.clone();
    if let Some((url, h)) = pick(cands, &job, &headers).await {
        if waiting(&t) {
            let name = {
                let mut d = t.d.lock();
                d.url = url;
                d.headers = h;
                d.stage.clear();
                d.status = Status::Queued;
                d.name.clone()
            };
            m.notice(serde_json::json!({"type": "refreshed", "id": id, "name": name}));
            return true;
        }
        return false;
    }
    fail(&m, &t, &job.err, "Sayfa yeniden açıldı ama aynı dosyaya giden bağlantı bulunamadı.");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// eski imza 403, farklı dosya yanlış boyut, yeni imza doğru boyut ama çerez istiyor
    #[tokio::test]
    async fn pick_finds_same_file_over_http() {
        use axum::{extract::Query, http::HeaderMap, routing::get, Router};
        async fn file(Query(q): Query<BTreeMap<String, String>>, h: HeaderMap) -> (axum::http::StatusCode, HeaderMap, Vec<u8>) {
            let mut out = HeaderMap::new();
            let sig = q.get("sig").map(String::as_str).unwrap_or("");
            let cookie_ok = h.get("cookie").and_then(|v| v.to_str().ok()).map(|c| c.contains("s=1")).unwrap_or(false);
            let len: u64 = match sig { "new" if cookie_ok => 1000, "other" => 999, _ => return (axum::http::StatusCode::FORBIDDEN, out, vec![]) };
            out.insert("content-range", format!("bytes 0-0/{len}").parse().unwrap());
            (axum::http::StatusCode::PARTIAL_CONTENT, out, vec![0])
        }
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, Router::new().route("/v/film.mp4", get(file)).route("/x/film.mp4", get(file))).await.unwrap() });
        let job = Job { id: "1".into(), page: format!("{base}/page"), url: format!("{base}/v/film.mp4?sig=old"), name: "film.mp4".into(), size: 1000, err: String::new(), taken: true };
        let c = |u: String, ck: bool| Cand { url: u, headers: if ck { [("Cookie".to_string(), "s=1".to_string())].into() } else { Default::default() } };
        let cands = vec![
            c(format!("{base}/v/film.mp4?sig=old"), true),
            c(format!("{base}/x/film.mp4?sig=other"), true),
            c(format!("{base}/v/film.mp4?sig=new"), false),
            c(format!("{base}/v/film.mp4?sig=new&c=1"), true),
        ];
        let mut old = BTreeMap::new();
        old.insert("Referer".to_string(), job.page.clone());
        let (u, h) = pick(cands, &job, &old).await.expect("aday bulunmalı");
        assert!(u.ends_with("sig=new&c=1"), "{u}");
        assert_eq!(h["Cookie"], "s=1");
        assert_eq!(h["Referer"], job.page);
        assert!(pick(vec![c(format!("{base}/x/film.mp4?sig=other"), true)], &job, &old).await.is_none());
    }

    #[test]
    fn expired_detection() {
        assert!(expired("Bağlantının süresi dolmuş olabilir (403)"));
        assert!(expired("Sunucu 410 döndü"));
        assert!(!expired("Sunucu 500 döndü"));
        assert!(!expired("Disk dolu"));
    }

    #[test]
    fn rank_prefers_same_filename_then_host() {
        let c = |u: &str| Cand { url: u.into(), headers: Default::default() };
        let r = rank(
            vec![c("https://other.net/x.bin"), c("https://cdn.a.com/zzz"), c("https://cdn2.a.com/v/Film.mp4?sig=new"), c("https://cdn2.a.com/v/Film.mp4?sig=new"), c("javascript:void(0)")],
            "https://cdn.a.com/v/film.mp4?sig=old",
            "film.mp4",
        );
        let u: Vec<_> = r.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(u, ["https://cdn2.a.com/v/Film.mp4?sig=new", "https://cdn.a.com/zzz", "https://other.net/x.bin"]);
    }
}
