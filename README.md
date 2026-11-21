# Trawl

Windows için indirme yöneticisi. Arka taraf Rust ve Tauri 2, arayüz Svelte 5. Dosyaları birden çok bağlantıyla indiriyor, yt-dlp üzerinden video sitelerini de alıyor, yanında bir tarayıcı eklentisi var.

Neler var:
- dosya başına 1 ile 32 arası bağlantı, biten bağlantı en çok işi kalan parçayı bölüp devralıyor
- parça durumu 3 sn'de bir diske yazılıyor, uygulama çökse de kaldığı yerden devam ediyor
- yt-dlp ile YouTube, TikTok, Instagram, X, Twitch ve yt-dlp'nin desteklediği diğer siteler (kalite seçimi, MP4/MKV/WEBM, MP3/M4A/OPUS/FLAC/WAV, altyazı ve bölüm gömme, SponsorBlock, oynatma listeleri)
- sayfa tarayıcı: video/audio etiketleri, linkler, srcset, og/twitter meta, JSON-LD, iframe'ler, script içine gömülü m3u8/mp4 adresleri. boyutları yokluyor, aynı sitede 1-2 seviye derine inebiliyor
- kuyruk, aynı anda indirme sınırı, genel hız sınırı, hepsi bitince uyku/kapat/çık
- zamanlayıcı, proxy (http, socks5), biten dosyada MD5/SHA-1/SHA-256 kontrolü, yt-dlp günde bir kendini güncelliyor
- sistem tepsisi, pano izleme, sürükle bırak, bildirimler, Windows ile başlama, tek örnek
- tarayıcı eklentisi (`extension/`): ağdaki medyayı yakalıyor (HLS/DASH dahil), Chrome indirmelerini devralıyor, sağ tık menüsü, YouTube'da İndir düğmesi (Shift ile direkt video, Alt ile direkt ses)

## Çalıştırma

```powershell
npm install
npx tauri dev
```

Kurulum dosyası için `npx tauri build`. Çıktı `src-tauri/target/release/bundle/nsis/Trawl_*_x64-setup.exe` altına düşüyor.

Arayüzü tek başına tarayıcıda sahte veriyle açmak için `npx vite`, sonra http://localhost:1420

## Bileşenler

yt-dlp, ffmpeg (LGPL derlemesi) ve deno uygulamadan kuruluyor: Ayarlar > Bileşenler. `%APPDATA%\Trawl\bin` içine iniyorlar. PATH'te zaten varsa onları kullanıyor.

## Eklenti

`chrome://extensions` aç, Geliştirici modunu aç, "Paketlenmemiş öğe yükle" ile `extension` klasörünü seç. Uygulamayla `127.0.0.1:47321` üzerinden konuşuyor, oraya sadece eklentiden gelen istekler kabul ediliyor.

## Yapı

| Dosya | Ne yapıyor |
|---|---|
| `src-tauri/src/http.rs` | parçalı HTTP indirme |
| `src-tauri/src/media.rs` | yt-dlp |
| `src-tauri/src/scrape.rs` | sayfa tarayıcı |
| `src-tauri/src/manager.rs` | kuyruk ve kayıt |
| `src-tauri/src/bridge.rs` | eklenti köprüsü (axum) |
| `src/` | arayüz |

## Notlar

- yt-dlp Unlicense, ticari kullanımda sorun yok. ffmpeg LGPL derlemesi, ayrı exe olarak çağrılıyor.
- YouTube'dan indirmek YouTube'un kullanım koşullarına aykırı olabilir, "YouTube indirici" diye öne çıkarmamak lazım.
- Kod imzalama sertifikası yokken Windows SmartScreen uyarı veriyor.
