# Kurulum Ve İstemci Ayarı

**AGZ Yazılım ürünüdür.** Bu belge
[belge okuma yolunun](README.tr.md) 1. adımıdır. Tüm kurulum yöntemlerini,
işletim sistemi notlarını, MCP istemci yapılandırmasını, doğrulamayı ve sorun
gidermeyi kapsar. Sonraki adım
[araç ve yapılandırma referansıdır](tools.tr.md).

`agz-rust-mcp`, standart stdio ve yerel Streamable HTTP MCP taşımalarını sunan
tek bir çalıştırılabilir dosyadır. Ajan harness'ı, model sağlayıcısı, istemci
eklentisi veya ayrı bir skill paketi gerektirmez. Aşağıdaki yöntemlerin tümü aynı
dosyayı kurar.

## Yerleşik skilller

Dört sürümlenmiş `SKILL.md` dosyası binary içine gömülüdür ve çevrimdışı çalışır:
`agz-rust-workflow`, `agz-rust-repair`, `agz-rust-refactor` ve
`agz-rust-performance`. İçerik, bağlantı kurulduğunda hemen kullanılabilir.

| MCP arayüzü | Keşif | Tek skill okuma |
| --- | --- | --- |
| Yalnız araçları destekleyenler dahil tüm araç istemcileri | `workspace(action="skills")` | `workspace(action="skills", skill="agz-rust-workflow")` |
| Prompt'lar | `prompts/list` | `name="workflow"` ile `prompts/get` |
| Kaynaklar | `resources/list` | `uri="agz-rust-mcp://skills/agz-rust-workflow"` ile `resources/read` |

Araç tam skill adını veya kısa prompt adını (`workflow`, `repair`, `refactor`,
`performance`) kabul eder. Katalog sürüm ve SHA-256 bilgisi taşır; seçilen içerik
`line`, `lineCount` ve `byteBudget` ile sayfalanabilir. Skill okumak proje kökü
istemez, Cargo/Rust Analyzer başlatmaz. MCP prompt'ları isteğe bağlı en fazla
4.000 temizlenmiş karakterlik `task` metni alır. Talimatların ne zaman seçileceğini
istemci belirler; MCP kaynak desteği otomatik çağrı anlamına gelmez.

CLI ile inceleme ve dışa aktarma isteğe bağlıdır:

```bash
agz-rust-mcp skills list
agz-rust-mcp skills show agz-rust-workflow
# Üst dizin mevcut ve güvenilir olmalı; hedef dizin mevcut olmamalı:
agz-rust-mcp skills export --dir /path/to/new-skills
```

Dışa aktarma her skill için bir klasör ve `SKILL.md` üretir; mevcut hedefi
reddeder ve hata durumunda kısmi çıktıyı bildirir. İstemci ayarlarını değiştirmez,
hiçbir istemcinin ayar dizinine kurulum yapmaz. Bu CLI komutları MCP/Cargo
başlatmaz ve sunucu ayarlarını okumaz. Binary güncellendiğinde yerleşik talimatlar
da güncellenir; önceki dışa aktarımlar anlık kopyadır ve ayrıca yenilenmelidir.

## Gereksinimler

- Linux, macOS veya Windows (x86_64) ya da macOS (arm64).
- Yerel stdio MCP veya yerel Streamable HTTP MCP destekleyen bir istemci.
- Bu sunucuyu `cargo install` veya kaynaktan derlemek için Rust `1.91.0` veya üzeri.
- Hazır sunucu binary'si kullanıldığında da metadata, derleyici tanıları, testler
  ve binary/example çalıştırma için Cargo ile hedef projenin Rust toolchain'i.
- Yalnız semantik gezinme ve düzenleme önerileri için Rust Analyzer. Nextest ve
  sccache isteğe bağlı hızlandırıcılardır; normal Cargo akışı kullanılabilir.

## 1. Kurulum Betiği (`install.sh`)

Kurulum betiği bir sürüm arşivini indirir, yayımlanmış SHA-256 sağlama
toplamını çıkarma öncesinde doğrular, symlink hedeflerini reddeder, hazırlanan
binary'yi doğrular ve atomik olarak kurar. Şu anda yalnızca **Linux x86_64**
destekler; diğer platformlar açık bir hata alır ve 2, 3 veya 4. yöntemi
kullanmalıdır.

Kurulum betiğini ve sağlama toplamı manifestini sürüm sayfasından indirin,
betiğin kendisini doğrulayın ve çalıştırın:

```bash
curl -fsSL -O \
  https://github.com/agzyazilim/rust-mcp/releases/latest/download/install.sh
curl -fsSL -O \
  https://github.com/agzyazilim/rust-mcp/releases/latest/download/SHA256SUMS
grep ' install.sh$' SHA256SUMS | sha256sum --check
# macOS: grep ' install.sh$' SHA256SUMS | shasum -a 256 -c -

bash install.sh
```

Betik sağlama toplamı doğrulaması başarısızsa çalıştırmayın; sürüm sayfasından
yeniden indirin. Ortam değişkenleri:

| Değişken | Varsayılan | Anlam |
| --- | --- | --- |
| `AGZ_RUST_MCP_VERSION` | kurulum betiğinin sürümü (bu kaynakta `0.6.0`) | Kurulacak tam sürüm. |
| `AGZ_RUST_MCP_INSTALL_DIR` | `$HOME/.local/bin` | Mutlak kurulum dizini. |

```bash
AGZ_RUST_MCP_VERSION=0.6.0 AGZ_RUST_MCP_INSTALL_DIR="$HOME/.local/bin" bash install.sh
```

Betik ayrıca arşiv düzenini ve çıkarılan binary'nin `--version` çıktısını
mevcut dosyayı değiştirmeden önce doğrular. `SHA256SUMS` dosyası crate, kaynak
paketi ve kurulum betiğini kapsar; her arşivin kendi `.tar.gz.sha256` dosyası
vardır.

## 2. Hazır Derlenmiş Arşivler

Sürüm sayfaları `.tar.gz` arşivlerini eşleşen `.sha256` dosyasıyla sunar:

| Platform | Arşiv (0.3.0 ve sonrası) |
| --- | --- |
| Linux x86_64 | `agz-rust-mcp-linux-x86_64.tar.gz` |
| macOS arm64 | `agz-rust-mcp-macos-arm64.tar.gz` |
| Windows x86_64 | `agz-rust-mcp-windows-x86_64.tar.gz` |

**0.1.0-0.2.0 eski varlıkları.** `0.2.0` dahil o sürüme kadarki release'ler
önceki ürün adıyla yayımlandı ve eski arşiv/çalıştırılabilir adlarını kullanır.
Dosyalar erişilebilir kalır ve `install.sh` bunları otomatik eşler:

| Sürüm hattı | Arşiv adı | Arşivdeki çalıştırılabilir |
| --- | --- | --- |
| `0.1.0`-`0.2.0` | `agz-rust-coder-<platform>-<arch>.tar.gz` | `agz-rust-coder` |
| `0.3.0` ve sonrası | `agz-rust-mcp-<platform>-<arch>.tar.gz` | `agz-rust-mcp` |

Arşiv adı ne olursa olsun kurulan çalıştırılabilir her zaman `agz-rust-mcp`
olur; `install.sh` eski arşivdeki çalıştırılabilir dosyayı kurulum sırasında
yeniden adlandırır.

Linux x86_64:

```bash
sha256sum --check agz-rust-mcp-linux-x86_64.tar.gz.sha256
tar -xzf agz-rust-mcp-linux-x86_64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 agz-rust-mcp "$HOME/.local/bin/agz-rust-mcp"
```

macOS arm64:

```bash
shasum -a 256 -c agz-rust-mcp-macos-arm64.tar.gz.sha256
tar -xzf agz-rust-mcp-macos-arm64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 agz-rust-mcp "$HOME/.local/bin/agz-rust-mcp"
```

Windows PowerShell:

```powershell
$archive = "agz-rust-mcp-windows-x86_64.tar.gz"
$expected = ((Get-Content "$archive.sha256" -Raw) -split '\s+')[0].ToLowerInvariant()
$actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expected -notmatch '^[a-f0-9]{64}$' -or $actual -ne $expected) {
    throw "Release archive checksum mismatch"
}
tar -xzf $archive
# Çıkarma klasörünü PATH'e ekleyin.
```

Komutlar arşivi çıkarmadan önce `.sha256` dosyasıyla doğrular.
Doğrulama başarısız olursa devam etmeyin.

## 3. crates.io (`cargo install`)

Rust `1.91.0` veya üzeri gerekir:

```bash
rustup toolchain install 1.91.0
cargo install agz-rust-mcp --locked
agz-rust-mcp --version
```

`--locked`, `Cargo.lock` içinde kayıtlı tam bağımlılık sürümleriyle derler.
Belirli bir sürüm için `cargo install agz-rust-mcp --version 0.6.0 --locked`
kullanın. Cargo `$HOME/.cargo/bin` (Windows'ta `%USERPROFILE%\.cargo\bin`)
dizinine kurar; bu dizinin `PATH` içinde olduğundan emin olun.

## 4. Kaynaktan Derleme

```bash
git clone https://github.com/agzyazilim/rust-mcp
cd rust-mcp
cargo +1.91.0 build --release -p agz-rust-mcp --locked
./target/release/agz-rust-mcp --version
```

MSRV, edition 2024 ile Rust `1.91.0` sürümüdür; `rust-toolchain.toml` CI'ın
kullandığı toolchain'i sabitler. `target/release/agz-rust-mcp` (Windows'ta
`.exe`) dosyasını `PATH` içindeki bir dizine kopyalayın veya istemciyi mutlak
yola yönlendirin. Tam geliştirme kapısı için
[CONTRIBUTING.md](../CONTRIBUTING.md) dosyasına bakın.

## İşletim Sistemi Notları

- **Linux.** En kısa doğrulanmış yol `install.sh` (yalnızca x86_64). Betik
  eksik olduğunu bildirirse `$HOME/.local/bin` dizinini `PATH`'e ekleyin.
- **macOS.** `macos-arm64` arşivini veya `cargo install` kullanın. Intel
  Mac'lerde bu sürümde hazır arşiv yoktur; kaynaktan derleyin. İşletim sistemi
  imzasız binary'yi engellerse System Settings -> Privacy & Security
  bölümünden izin verin.
- **Windows.** `windows-x86_64` arşivini veya `cargo install` kullanın;
  `install.sh` Windows'u desteklemez. Çıkarma dizinini `PATH`'e ekleyin.

## MCP İstemci Ayarı

Bağlanmadan önce `agz-rust-mcp --version` çalıştırın. İstemcinin `PATH` değeri
geliştirme kabuğunuzdan farklıysa mutlak binary yolu verin; sunucu hedef projenin
Rust/Cargo toolchain'ine erişebilmelidir. Aşağıdaki başlatma alanlarını istemcinizin
kendi yapılandırma biçimine aktarın; bunlar evrensel bir istemci ayar dosyası değildir.

### Stdio

```json
{
  "command": "agz-rust-mcp",
  "args": ["--allow-root", "/absolute/path/to/project"],
  "env": {"AGZ_RUST_MCP_SERVER__TOOLSET": "core"}
}
```

Varsayılan taşıma stdio'dur. İstemci süreci başlatır ve stdin/stdout üzerinde
satır sonlu JSON-RPC alışverişi yapar. Tanılar stderr'e gider. Shell sarmalayıcısı
veya model API anahtarı gerekmez.

### Streamable HTTP

Süreci ayrıca başlatın:

```bash
agz-rust-mcp --transport http --http-bind 127.0.0.1:8787 \
  --allow-root /absolute/path/to/project
```

İstemcinin Streamable HTTP bağlantısında şu URL'yi kullanın:

```json
{
  "url": "http://127.0.0.1:8787/mcp"
}
```

Aynı uç nokta `2025-11-25` oturumlu istemcilerini ve durumsuz `2026-07-28`
protokolünü destekler. Dinleyici yalnız loopback adreslerine bağlanır; yerel native
istemci doğrudan bağlanır. `Origin` başlığı taşıyan istekler reddedilir; `Host`,
dinleyici adresi veya aynı portla `localhost` olmalıdır. Bu nedenle tarayıcı
sayfası script'leri yerel servisi doğrudan çağıramaz.

Bir sunucu süreci tek güvenilir kullanıcının workspace alanıdır: change,
capsule ve task durumu yapılandırılmış erişim ve yaşam süresini paylaşır. Uzak
kullanım, `Host` değerini loopback dinleyicisine dönüştüren kimlik doğrulamalı
HTTPS proxy gerektirir; ayrı yetki sınırı olan her kullanıcı için ayrı sunucu
süreci kullanın. HTTP-stdio köprüsüne gerek yoktur.

Aynı ayarlar sunucunun TOML dosyasında da tanımlanabilir:

```toml
[server]
transport = "http"
http_bind = "127.0.0.1:8787"
allow_roots = ["/absolute/path/to/project"]
toolset = "core"
```

Dosyayı `agz-rust-mcp --config /absolute/path/to/server.toml` ile yükleyin. Öncelik
sırası CLI bayrakları, `AGZ_RUST_MCP_*` ortam değişkenleri, açık TOML dosyası ve
varsayılanlardır. Ortam değişkeni karşılıkları
`AGZ_RUST_MCP_SERVER__TRANSPORT=http` ve
`AGZ_RUST_MCP_SERVER__HTTP_BIND=127.0.0.1:8787` şeklindedir.

### Araç kataloğu ve proje kökleri

Küçük `core` kataloğu `workspace`, `test`, `run`, `check`, `crate_lookup` ve `docs`
araçlarını içerir. `--toolset core` veya `AGZ_RUST_MCP_SERVER__TOOLSET=core` ile
seçilir. Varsayılan `full` kataloğu gelişmiş semantik, değişiklik, onarım, doğrulama
ve profil araçlarını da sunar. Açık araç etkinleştirme/kapatma ayarları seçilen
kataloğu daraltır; `core` içine gelişmiş araç eklemez. Katalog değişince sunucuyu
yeniden başlatın.

Kanonik çalışma dizini varsayılan yetkili köktür. Ek checkout veya worktree'ler
için tekrarlanan `--allow-root` argümanları verin; her araç çağrısında mutlak `dir`
kullanın. Harici path bağımlılıkları açık `--allow-dependency-root` gerektirir.
Örneğin:

```text
--allow-root /projects --allow-root /client-worktrees --allow-dependency-root /projects
```

İstemcinin bildirdiği MCP kökleri yapılandırılmış erişimi daraltabilir,
genişletemez. Paket/kaynak alt dizinlerinin miras aldığı bağımlılıklar seçilen
workspace üzerinden çözülür. Farklı checkout'lar ayrı cache anahtarlarını korur.
Auto cache modunda güvensiz veya harici target dizini yalıtılmış target'a yönlenir;
project-only modu güvenli workspace içi target gerektirir. Eksik veya erişilemeyen
girdiler başarılı sonuç yerine sınırlı bir neden döndürür.

`tools/list` yanıtındaki tam araç adlarını kullanın; istemcinin ekranda eklediği
ad alanı istemciye aittir. Tüm ayar ve varsayılanlar için
[yapılandırma referansına](tools.tr.md) bakın.

## Doğrulama

1. Çalıştırılabilir dosyayı denetleyin:

   ```bash
   agz-rust-mcp --version
   agz-rust-mcp --help
   ```

2. Stdio üzerinden MCP el sıkışmasını ve araç kataloğunu denetleyin (satır
   sonlu JSON-RPC):

   ```bash
   {
     printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"verify","version":"1.0.0"}}}'
     printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
     printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
   } | agz-rust-mcp
   ```

   `initialize` sonucunda uzlaşılan protokol adını, `tools/list` sonucunda
   `workspace`, `test`, `run`, `check`, `profile`, `audit`, `crate_lookup`, `docs`, `context`, `api`,
   `explain`, `verify`, `symbol`, `references`, `definition`, `symbols`,
   `implementations`, `hierarchy`, `rename`, `refactor`, `change`, `repair` ve
   `work` araçlarını bekleyin (yapılandırmayla kapatılan araçlar listelenmez).
   Stdout yalnız MCP çerçevelerine aittir; tanılar stderr'e gider.

3. Kaynak kopyada depo protokol smoke'unu çalıştırın:

   ```bash
   cargo run -p xtask -- protocol-smoke
   ```

## İlk Geliştirme Görevi

Kütüphane, servis, komut satırı uygulaması veya çok paketli workspace için aynı
akışı kullanın:

| Amaç | Araç ve eylem | Kullanılacak sonuç |
| --- | --- | --- |
| Derlemeden projeyi keşfetmek | `workspace` → `inspect` | Manifestler, paketler, hedefler, feature'lar ve mevcut lockfile sürümleri. |
| İlgili kaynağı bulmak | `workspace` → `search`, ardından `read` | Sınırlı metin eşleşmeleri ve satır aralıkları; semantik referans değil metin aramasıdır. |
| İlk derleyici hatasını bulmak | Seçilen paket/hedef ile `check` | Gerçek Cargo/rustc tanıları ve kaynak konumları. |
| Bağımlılık kanıtını toplamak | `crate_lookup`, `docs` veya `context` | Gerçek bağımlılık sürümü ve kaynağına bağlı kanıt. |
| Regresyon testini listeleyip ayırmak | `test` → `list`, ardından `run` | Seçilen test adları ve yeni test sonuçları. Uygunsa tam ad kullanın. |
| Testleri çalıştırmadan derlemek | `test` → `compile` | Yalnız derleme kanıtı. |
| Fonksiyonu test/example üzerinden denemek | Açık binary/example hedefiyle `run` | Çıkış durumu, sınırlı çıktı, süre ve süreç temizleme durumu. |

Sıradan Rust fonksiyonları çalıştırılabilmek için test, example veya binary giriş
noktasına ihtiyaç duyar. `run`, Cargo hedefi ve yapılandırılmış argümanlar alır;
shell komutu almaz. Cargo build script'leri, testler ve programlar yerel proje
kodunu çalıştırır; dosya veya ağ işlemleri yapabilir. Sunucunun kök denetimleri
ve süreç sınırları, bu kod için işletim sistemi sandbox'ı değildir. Seçilen tam
kapsamı, kesilme bildirimlerini ve timeout/iptal durumunu inceleyin; eşleşen test
bulunmaması başarılı regresyon testi sayılmaz.

## Sorun Giderme

| Belirti | Denetim ve çözüm |
| --- | --- |
| `agz-rust-mcp: command not found` | Kurulum dizinini (`$HOME/.local/bin` veya `$HOME/.cargo/bin`) `PATH`'e ekleyin ya da istemci yapılandırmasında mutlak yol kullanın. |
| Sağlama toplamı uyuşmuyor | Arşivi ve `.sha256` dosyasını sürüm sayfasından yeniden indirin; doğrulamayı atlamayın. |
| Kurulum betiği işletim sistemini/arch'i reddediyor | `install.sh` yalnız Linux x86_64 destekler; hazır arşiv, `cargo install` veya kaynak derleme kullanın. |
| İstemci araç göstermiyor | Stdio için binary yolunu ve başlangıç süresini denetleyin. HTTP için sunucunun açık olduğunu doğrulayın ve `/mcp` URL'sini kullanın. |
| HTTP adresi reddediliyor | `--http-bind`, `127.0.0.1:8787` veya `[::1]:8787` gibi bir loopback adresi olmalıdır. |
| İstemci prompt/kaynak listesi göstermiyor | `workspace(action="skills")` ile talimatları okuyun; dosya tabanlı skill kurulumu gerekmez. |
| Yol veya kök yetki hatası | İstemciyi workspace içinde başlatın veya tekrarlanan `--allow-root <yol>` bayrakları verin. |
| Semantik araçlar erişilememe döndürüyor | Sabitlenmiş Rust Analyzer'ı kurun (`rustup component add rust-analyzer --toolchain 1.91.0`) ve workspace-code politikasını [docs/tools.tr.md](tools.tr.md) içinden inceleyin. |
| Eski toolchain'de `cargo install` başarısız | `rustup toolchain install 1.91.0` ile Rust `1.91.0` veya üzerini kurun. |

İlgili: [belge dizini](README.tr.md) - [araç referansı](tools.tr.md) -
[mimari](architecture.tr.md) - [güvenlik politikası](../SECURITY.md).
