# AGZ Rust MCP

[![CI](https://github.com/agzyazilim/rust-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/agzyazilim/rust-mcp/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/agz-rust-mcp.svg)](https://crates.io/crates/agz-rust-mcp)
[![Lisans: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md) | Türkçe

**AGZ Yazılım ürünüdür.** `agz-rust-mcp`, Cargo ve rustc çıktısına dayanan
genel amaçlı Rust geliştirme için stdio ve Streamable HTTP taşımalı bağımsız bir
MCP sunucusudur. Projeyi
çevrimdışı keşfeder, ilgili kaynak bağlamını ve derleyici hatalarını toplar,
seçili testleri ve binary/example hedeflerini çalıştırır. Tam sürümlü bağımlılık
belgeleri, Rust Analyzer ve kaynağa yazmayan değişiklik paketleri derin
inceleme ve refactor çalışmalarını destekler.

## Günlük geliştirme akışı

Aynı akış kütüphane, CLI, servis veya çok paketli Cargo workspace için
kullanılır. Belirli bir framework ya da MCP istemcisi gerekmez.

| İhtiyaç | Araç çağrısı |
| --- | --- |
| Derleme başlatmadan paket, hedef, feature ve bağımlılık bildirimlerini gör | `workspace(action="inspect")` |
| Kodu bul ve sınırlı, satır numaralı kaynak oku | `workspace(action="search", query="parse_record")`, ardından `workspace(action="read", path="src/lib.rs", line=1, lineCount=80)` |
| Güncel derleyici hatalarını al | `check(target="check", options={"packages":["my_crate"]})` |
| Gerçek test adlarını keşfet | `test(action="list", packages=["my_crate"])` |
| Tam adıyla tek kütüphane testi çalıştır | `test(action="run", packages=["my_crate"], cargoTarget={"kind":"lib"}, filter="tests::parses_record", exact=true)` |
| Testleri çalıştırmadan derle | `test(action="compile", packages=["my_crate"])` |
| Binary veya example çalıştır | `run(package="my_crate", target={"kind":"example","name":"demo"}, args=["input.json"], timeoutMs=30000)` |
| Bilinmeyen bağımlılık API'sini çöz | Tam sürümüyle `docs`; derin çözümleme/deneme için `api` |

İstemci başka dizinden açılıyorsa etkin projenin mutlak `dir` yolunu verin.
Fonksiyon test veya çalıştırılabilir example üzerinden yürütülür. `run`, tipli
Cargo çalıştırıcısıdır; boşluk ve shell karakterleri içeren program argümanları
aynen aktarılır. Test ve uygulama kodunun normal yan etkileri olabilir.
Boş seçimler başarılı test yerine `NO_TESTS` döndürür.

Altı günlük araçtan oluşan küçük katalog için `agz-rust-mcp --toolset core`
kullanın: `workspace`, `check`, `test`, `run`, `docs`, `crate_lookup`.
Varsayılan `--toolset full`, mevcut araçların tamamını ve yeni eklemeleri sunar.
`--tools-<name> false` seçenekleri araçları ayrıca kapatabilir. Sınırlar ve
sonuç durumları için [tam referansı](docs/tools.tr.md#gunluk-rust-akisi) okuyun.

## Kurulum

```bash
# kurulum betiği (Linux x86_64): install.sh ve SHA256SUMS dosyalarını en güncel
# sürümden indirin, betiği doğrulayın, sonra çalıştırın
bash install.sh

# crates.io (Rust 1.91.0 veya üzeri gerekir)
cargo install agz-rust-mcp --locked
agz-rust-mcp --version
```

Linux x86_64, macOS arm64 ve Windows x86_64 için `.sha256` dosyalı hazır
`.tar.gz` arşivleri vardır. `0.2.0` dahil o sürüme kadarki release'ler eski
`agz-rust-coder-*` varlık adlarını kullanır; `0.3.0` sürümünden itibaren adlar
`agz-rust-mcp-*` olur.

**Adım adım yönergeler, işletim sistemi notları, sağlama toplamı doğrulaması ve
sorun giderme: [docs/install.tr.md](docs/install.tr.md) ·
[English](docs/install.md).**

## MCP İstemci Ayarı

Yerel stdio MCP destekleyen istemcinizin kendi sunucu yapılandırma biçiminde
kurulu çalıştırılabilir dosyayı aşağıdaki alanlarla tanımlayın:

```json
{
  "command": "agz-rust-mcp",
  "args": ["--allow-root", "/absolute/path/to/project"],
  "env": {"AGZ_RUST_MCP_SERVER__TOOLSET": "core"}
}
```

HTTP kullanan istemci için aynı binary'yi yerel Streamable HTTP sunucusu olarak
başlatın ve MCP URL'sini `http://127.0.0.1:8787/mcp` olarak tanımlayın:

```bash
agz-rust-mcp --transport http --http-bind 127.0.0.1:8787 \
  --allow-root /absolute/path/to/project
```

HTTP dinleyicisi yalnız loopback adreslerine bağlanır. `2025-11-25` istemcilerinin
oturum yaşam döngüsünü ve durumsuz `2026-07-28` protokolünü destekler. Görünen araç
önekleri, yapılandırma dosyası konumu ve otomatik prompt seçimi istemciye aittir;
sunucu bunlara bağımlı değildir. Kanonik çalışma dizini varsayılan yetkili köktür;
istemcinin bildirdiği kökler yapılandırılmış erişimi yalnız daraltabilir.
[Bağlantı ayrıntıları](docs/install.tr.md#mcp-istemci-ayarı).

## Yerleşik skilller

Çalıştırılabilir dosyada dört çevrimdışı skill bulunur: `agz-rust-workflow`,
`agz-rust-repair`, `agz-rust-refactor` ve `agz-rust-performance`.
Üç standart MCP arayüzünden hemen kullanılabilirler:

- Araçlar: `workspace(action="skills")` kataloğu listeler;
  `workspace(action="skills", skill="agz-rust-workflow")` içeriği okur.
- Prompt'lar: `prompts/list` ve `prompts/get`, `workflow`, `repair`,
  `refactor` ve `performance` akışlarını sunar.
- Kaynaklar: `resources/read`, `agz-rust-mcp://skills/<skill-name>` adresini okur.

Yalnız araçları gösteren bir istemci de tüm skill içeriğine erişir. Sürüm ve
SHA-256 bilgileri kullanılan yerleşik talimatları tanımlar. Skill kurulumu veya
ayrı bir MCP gerekmez. İsteğe bağlı `agz-rust-mcp skills export --dir <new-directory>`
komutu dosya skilllerini destekleyen istemciler için çıktı üretir; mevcut
dosyaların üzerine yazmaz. [Yerleşik skilller](docs/install.tr.md#yerleşik-skilller).

## Belgeler

Belgeleri şu sırayla okuyun:

1. [Kurulum ve istemci ayarı](docs/install.tr.md) - tüm kurulum yöntemleri ve
   istemci yapılandırması.
2. [Araç ve yapılandırma referansı](docs/tools.tr.md) - araçlar, eylemler,
   sonuç anlamları ve tüm yapılandırma anahtarları.
3. [Mimari](docs/architecture.tr.md) - süreç modeli, protokol yaşam döngüsü ve
   yetki sınırları.
4. [Doğrulama ve benchmark protokolü](docs/benchmark.tr.md) - smoke'lar, kapılar
   ve kanıt düzeni.
5. [Güvenlik politikası](SECURITY.md) ve [Katkı](CONTRIBUTING.md).

Tam okuma yolunu içeren iki dilli dizin:
[docs/README.tr.md](docs/README.tr.md) / [docs/README.md](docs/README.md).

## Kimlik

| Sözleşme | Değer |
| --- | --- |
| Crate, binary, server | `agz-rust-mcp` |
| MCP Registry | `io.github.agzyazilim/rust-mcp` |
| Kaynak sürümü | `0.6.0` |
| İlk sürüm | `0.1.0` |
| Release tag | `agz-rust-mcp-v<version>` |
| Rust edition / MSRV | `2024` / `1.91.0` |
| Rust MCP SDK | `rmcp` `3.8.0` |
| Varsayılan / keşfedilen protokol | `2025-11-25` / `2026-07-28` |

MCP paket sahipliği kaydı: `mcp-name: io.github.agzyazilim/rust-mcp`.

## Araçlar

Aşağıdakiler sunucunun tam araç adlarıdır. `tools/list` yanıtındaki adları
kullanın; istemci görünümde kendi ad alanını veya önekini ekleyebilir.

| MCP tool | Default | Amaç |
| --- | --- | --- |
| `workspace` | `enabled` | Yerleşik skillleri oku, manifest/lockfile incele, çevrimdışı kaynak ara ve kesit oku. |
| `test` | `enabled` | Tam ad, ignored-test seçenekleri ve sınırlı süreyle test keşfet, derle veya çalıştır. |
| `run` | `enabled` | Tam seçilmiş Cargo binary/example hedefini gerçek argümanlar, süre ve çıktı sınırıyla çalıştır. |
| `check` | `enabled` | Sınırlı Cargo check, Clippy, test, docs veya tam kapıyı çalıştırır. |
| `profile` | `enabled` | Gözlenen Cargo yeniden derleme davranışını analiz eder ve ölçülmemiş hız iddiası kurmadan sınırlı derleme kanıtını karşılaştırır. |
| `audit` | `enabled` | Rust kaynağını sınırlı statik bulgular için tarar. |
| `crate_lookup` | `enabled` | Crate adını ve isteğe bağlı tam sürümü crates.io üzerinde doğrular. |
| `docs` | `enabled` | Tam sürüm belgesini cache, yerel kaynak veya docs.rs üzerinden çözer. |
| `context` | `enabled` | Öğe başına nedeniyle revizyona bağlı anlamsal bağlam kapsülü hazırlar, genişletir veya farkını üretir. |
| `api` | `enabled` | Sınırlı analiz kanıtıyla API imzasını çözer veya aday kod parçasını workspace yapılandırmasının yalıtılmış kopyasında tip kontrolünden geçirir. |
| `explain` | `enabled` | Makro açılım kaynağını, trait yükümlülüklerini veya cfg etkinliğini açıklar. |
| `verify` | `enabled` | Sınırlı feature, hedef, toolchain ve aşama matrisini planlar veya çalıştırır. |
| `symbol` | `enabled` | Bir sembol için Rust Analyzer hover verisini okur. |
| `references` | `enabled` | Sınırlı referansları bulur. |
| `definition` | `enabled` | Seçilen tanımı bulur. |
| `symbols` | `enabled` | Bir Rust dosyasındaki sembolleri listeler. |
| `implementations` | `enabled` | Uygulamaları bulur. |
| `hierarchy` | `enabled` | Sınırlı çağrı hiyerarşisini izler. |
| `rename` | `enabled` | Uygulamadan doğrulanmış yeniden adlandırma paketi üretir. |
| `refactor` | `enabled` | Uygulamadan doğrulanmış refactor paketi üretir. |
| `change` | `enabled` | Workspace'e yazmadan sunucuya ait scratch alanında revizyona bağlı changeset oluşturur, uygular, göç ettirir ve doğrular. |
| `repair` | `enabled` | Başarısız bir change revizyonu için derleyici güdümlü onarım adaylarını analiz eder, dener, karşılaştırır ve küçültür; workspace'e yazmaz. |
| `work` | `enabled` | Tipli bir intent'i açık kapılar ve bütçelerle change/validate üzerinden yürütür; dürüst kapı kanıtı veya sınırlı tek kullanımlık handoff döndürür. |

Her araç belirli yapıda veri ve ona eşdeğer, boyutu sınırlı metin döndürür. Dış
veri `untrustedData` altında tutulur. Derleme hatası, bulunamayan crate veya
erişilemeyen belge gibi beklenen sonuçlar tipli sonuçtur; geçersiz girdi, yetki
ihlali, kaynak tükenmesi ve semantik altyapı yokluğu protokol hatasıdır.

## Yapılandırma

Öncelik sırası CLI, `AGZ_RUST_MCP_*` ortam değişkenleri, açık `--config` TOML
dosyası ve varsayılanlardır. Ortam anahtarları bölümler arasında `__` kullanır;
örnek: `AGZ_RUST_MCP_GATE__HARD_TIMEOUT_MS=600000`.

| Key | Default | Anlam |
| --- | --- | --- |
| `server.transport` | `stdio` | MCP taşıması: `stdio` veya `http`. |
| `server.http_bind` | `127.0.0.1:8787` | HTTP `/mcp` uç noktası için loopback adresi. |
| `server.allow_roots` | canonical CWD | Workspace okuma/komut sınırı. |
| `server.allow_dependency_roots` | empty | Açık dış path-dependency kökleri. |
| `gate.hard_timeout_ms` | `600000` | Cargo işlemi son süresi. |
| `gate.scope` | `shadow` | Doğrulama hedefi: `workspace`, `shadow` veya `affected`. |
| `gate.cache` | `auto` | Cache politikası: `auto`, `project` veya `isolated`. |
| `rust_analyzer.workspace_code` | `deny` | Workspace kodu kapatılamazsa RA başlatmayı reddeder. |
| `docs.fallback` | `auto` | Belge kaynağı politikası. |
| `profile.max_report_bytes` | `4194304` | Tek Cargo zamanlama artifact'ı için sınırlı okuma/saklama üst sınırı. |
| `profile.max_runs` | `4` | Bir `profile` çağrısında kullanılabilen taze Cargo koşusu. |
| `profile.compare_samples` | `3` | Hız iddiası öncesi her taraf için gereken örnek sayısı. |
| `limits.tool_output_bytes` | `49152` | Serileştirilmiş araç sonucu üst sınırı. |
| `change.max_bytes` | `268435456` | Changeset başına yakalanan aday byte üst sınırı. |
| `repair.max_candidates` | `4` | Bir `repair` işleminin deneyebileceği aday sayısı. |
| `repair.max_compiles` | `4` | Bir `repair` işleminin çalıştırabileceği Cargo doğrulaması. |
| `repair.wall_time_ms` | `120000` | Bir `repair` işlemi için duvar saati bütçesi. |
| `repair.minimize_max_candidates` | `32` | Bir `repair(action=minimize)` işleminin derlemeyle değerlendirebileceği küçültme denemesi. |
| `repair.minimize_max_compiles` | `16` | Bir `repair(action=minimize)` işleminin yeniden üretim ve dışa aktarma doğrulaması dahil çalıştırabileceği Cargo koşusu. |
| `work.max_candidates` | `4` | Bir work öğesinin stage edebileceği host aday revizyonu. |
| `work.max_compiles` | `12` | Bir work öğesinin çalıştırabileceği kapı doğrulaması. |
| `work.wall_time_ms` | `600000` | Bir work öğesi için duvar saati bütçesi. |
| `telemetry.enabled` | `true` | Prompt veya kaynak içermeyen sınırlı yerel etkinlik kaydı. |

`profile` kanıtı sınırlıdır: en güncel 64 kayıt bellekte tutulur ve sunucuya ait
`profile-evidence` dizinindeki zamanlama artifact'ları 24 saat sonra veya 64
dosyayı aşınca temizlenir. `profile` sonucu bu saklama politikasını görünür
kılar; süresi dolan kanıt sessizce yeniden kullanılmaz.

Tüm CLI alanları için `agz-rust-mcp --help` çalıştırın. Tam davranış ve
varsayılan tablosu [docs/tools.tr.md](docs/tools.tr.md) içindedir.

## Güvenlik

Sunucu workspace kaynağını değiştirmez, ancak işletim sistemi sandbox'ı
değildir. Cargo build script'leri, testler, procedural macro'lar, yerel rustdoc
ve açıkça etkinleştirilen Rust Analyzer workspace kodu sunucu kullanıcısının
yetkileriyle çalışır. Daha güçlü sınır gerektiğinde container veya OS sandbox
kullanın. Workspace, dependency, cache, lease, journal, docs ve telemetry
yolları kanonikleştirilir, sınırlandırılır ve şüphede kapalı kalır; stdout
yalnız MCP çerçevelerine ayrılmıştır. Güvenlik açıklarını [SECURITY.md](SECURITY.md)
uyarınca özel bildirin.

## Bağlantılar

- Repository: https://github.com/agzyazilim/rust-mcp
- Crate: https://crates.io/crates/agz-rust-mcp
- SDK docs: https://docs.rs/rmcp/3.8.0/rmcp/
- MCP `2025-11-25`: https://modelcontextprotocol.io/specification/2025-11-25
- MCP `2026-07-28`: https://modelcontextprotocol.io/specification/2026-07-28

## Lisans

[MIT](LICENSE), Copyright (c) 2026 Ugur Murat Altintas.
