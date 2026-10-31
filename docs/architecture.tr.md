# Mimari

**AGZ Yazılım ürünüdür.** Bu belge
[belge okuma yolunun](README.tr.md) 3. adımıdır:
[araç ve yapılandırma referansının](tools.tr.md) arkasındaki süreç modelini
açıklar. [Doğrulama ve benchmark protokolü](benchmark.tr.md) ile devam edin.

`agz-rust-mcp`; standart MCP stdio ve Streamable HTTP adaptörleri, sınırlı alan
servisleri ve denetimli dış süreçlerden oluşan tek bir Rust sürecidir. HTTP yalnız
loopback adresine bağlanır. Sunucu workspace kaynağına yazmaz.

## Veri Akışı

1. `main` katı yapılandırmayı yükler ve varsayılan olarak RMCP'yi stdin/stdout
   üzerinde başlatır; `server.transport="http"` ile `/mcp` HTTP uç noktasını sunar.
2. Sunucu varsayılan olarak `2025-11-25` protokolüyle uzlaşır ve
   `2026-07-28` desteğini keşfedebilir.
3. Client root'ları yapılandırılmış kök kümesini daraltır; her workspace ve
   dependency yolu root guard tarafından denetlenir.
4. Admission control eşzamanlı araçları ve task'ları sınırlar.
5. Alan servisleri Cargo, statik audit, belge veya Rust Analyzer işlemlerini
   ortak süreç denetimi üzerinden çalıştırır.
6. Yanıtlar tek wire-size sınırı içinde belirli yapıda veri ve eşdeğer metne
   dönüştürülür.
7. Kapanma yeni işi durdurur, task'ları iptal eder, Rust Analyzer'ı kapatır,
   denetimli süreç gruplarını/job'ları sonlandırır, telemetry'yi yazar ve eksik
   temizliği hata olarak bildirir.

## Bileşenler

| Component | Responsibility |
| --- | --- |
| `server` | MCP araçları, kaynaklar, prompt'lar, task'lar, ilerleme, yanıt eşitliği. |
| `transport` | Loopback Streamable HTTP, Host/Origin doğrulaması, servis ve gövde sınırları. |
| `skills` | Çalıştırılabilir dosyaya gömülü sürümlü çevrimdışı iş akışları; araçlar, prompt'lar, kaynaklar ve isteğe bağlı export. |
| `workspace` | Root yetkisi, paket seçimi, metadata, girdi kimliği. |
| `gate` | Cargo hedefleri, ön kontrol, singleflight, cache, host lease'leri. |
| `process` | Sınırlı çıktı, son süre, süreç grubu/Job Object, kurtarma journal'ı. |
| `lsp` | Rust Analyzer yaşam döngüsü, belge eşleme, gezinme, yazmasız edit. |
| `docs` | Lockfile çözümü, cache, source/docs.rs/yerel fallback. |
| `tools` | Doğrulama, audit, crate lookup, semantik alan işlemleri. |
| `telemetry` | Sınırlı yerel etkinlik kaydı ve atomik döndürme. |

## Protokol Yaşam Döngüsü

Sunucu tools, resources, prompts, roots, progress, cancellation ve tasks
yüzeylerini sunar. Task destekli istekler `CreateTaskResult` döndürür; polling ve
`tasks/cancel`, uzlaşılan RMCP task modelini kullanır. Task desteği olmayan
istemci aynı alan işlemini eşzamanlı alır.

Root değişikliği epoch değerini artırır. Eski root nesline bağlı iş iptal edilir
ve root'a duyarlı cache yetkisi geçersiz olur. Client root'ları kimlik doğrulama
değildir ve yapılandırılmış köklerde olmayan yolu yetkilendiremez.

HTTP, modern durumsuz isteklerin önceki task'a devam edebilmesi veya context
kapsülünü genişletebilmesi için alan durumunu süreç boyunca saklar. Her eski
protokol oturumu veya modern istek ayrı client-root koordinatörü kullanır.
Sınırlı yetki önbelleği, farklı kök kümelerinin birbirinin kanıtını geçersiz
kılmasını önler; istemcinin root-change bildirimi kendi önceki yetki neslini
geçersiz kılmayı sürdürür.

Her süreç tek güvenilir kullanıcının alanını sunar. Task kimlikleri ve araç
durumu bu alan içinde paylaşılır; HTTP oturumu ayrı kullanıcı hesabı değildir.
Her güvenlik kimliği için ayrı sunucu süreci kullanılır. Uzak erişim loopback
dinleyicisinin önünde kimlik doğrulamalı HTTPS proxy gerektirir. Tarayıcı Origin
başlıkları reddedilir; proxy kabul edilen yerel Host değerini iletmelidir.
Standart MCP SDK eski oturum kimliklerini ve modern istek metadata'sını yönetir.

## Yetki Modeli

Doğrulamayı Cargo ve rustc çıktısı belirler. Rust Analyzer ve kaynak audit'i
tavsiye niteliğindedir. Başarılı sonuç gözlenen worktree/girdi kimliğine bağlıdır
ve sonraki açık doğrulama isteğinde yeniden kullanılmaz.

Semantik rename/refactor yanıtları sınırlı edit paketleri içerir. Sunucu yol
sınırını, dosya sürümünü, çakışmayı ve eski metni doğrular ama değişikliği
uygulamaz.

## Süreç ve Depolama Sınırları

Unix komutları process group, Windows komutları Job Object kullanır. Timeout ve
kapanma önce nazik durdurmayı, ardından sınırlı zorla sonlandırma ve reap
işlemini dener. Temizliğin tamamlandığı kanıtlanamazsa journal kaydı tutulur.

Sunucuya ait durum, yetkili köklerin dışında platformun `agz-rust-mcp` ad
alanında yaşar. Cache yayını lock korumalı, aynı dizinde atomik değiştirme
kullanır. Symlink veya üst dizin kimliği belirsizliği şüphede kapalı kalır.

## Kalan Risk

Bu mimari sandbox değildir. Cargo build script'leri, testler, procedural
macro'lar, yerel rustdoc ve açıkça izin verilen Rust Analyzer workspace kodu
işletim sistemi hesabının dosya, süreç ve ağ haklarını kullanabilir. Unix alt
süreçleri kasıtlı olarak process group dışına çıkabilir. Daha güçlü güvence için
container veya OS sandbox gerekir.

## Dağıtım

Crate ve binary adı `agz-rust-mcp` değeridir. Release tag'leri
`agz-rust-mcp-v<version>` biçimindedir. Resmî MCP Registry kimliği
`io.github.agzyazilim/rust-mcp` olup tam crates.io paket sürümü ve
repository'deki `server.json` tarafından desteklenir.
