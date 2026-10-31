# 0.5.0 karar kaydı

## Doğrulanan başlangıç

Taban: `3892c5695207929abe8d23c4f0b51203e7376574`, sürüm 0.4.0.
Cargo tanıları, süreç ağacı yönetimi, kaynak erişim sınırları, sürümlü crate
belgeleri ve Rust Analyzer araçları mevcut. İlk proje keşfi Cargo metadata ve
önceden bilinen dosya/simge konumlarına bağımlı. Günlük test keşfi, tam adlı test
ve doğrudan binary/example çalıştırma araçları eksik. Iced yönlendirmesi ve
zorunlu eski OpenCode2 smoke bağımlılığı genel kullanım amacını daraltıyor.

## Son hedef

Herhangi bir Rust kütüphanesi veya Cargo workspace'inde ajanların kaynak
bağlamını, derleyici hatalarını, seçili testleri ve çalıştırılabilir hedefleri
aynı sınırlı, ölçülebilir ve istemciden bağımsız MCP sözleşmesiyle kullanması.

## Bağlayıcı kararlar

- Rust 2024, MSRV 1.88, `forbid(unsafe_code)`, mevcut ürün ve Registry kimliği
  korunur. Mevcut araçlar geriye uyumlu kalır.
- `workspace inspect/search/read` yalnız yetkili kaynakları okur; Cargo, ağ
  veya Rust Analyzer başlatmaz. Manifest bildirimi, lock sürümü ve çözümlenmiş
  derleyici bilgisi ayrı gösterilir. Metin araması semantik referans sayılmaz.
- `test list/run/compile` ve `run` yapılandırılmış Cargo argümanları kullanır.
  Paket/hedef seçimi doğrulanır. Süre sınırı hazırlığı da kapsar. Çıktı ve test
  listeleri sınırlıdır. Sıfır test, başarılı test kanıtı sayılmaz.
- `run` yalnız Cargo binary/example hedeflerini çalıştırır; keyfi shell veya
  executable kabul etmez. Bir fonksiyon test/example aracılığıyla yürütülür.
  Çalıştırılan proje kodu dosya/ağ yan etkileri yapabilir; araç OS sandbox değildir.
- Derleyici çıktısı doğruluk kaynağıdır. RA ve statik öneriler danışma verisidir.
  Her açık test/çalıştırma isteği yeni yürütme yapar; eski sonuç yeniden doğrulama
  yerine kullanılamaz. Yetki kökleri ve iptal/cleanup sözleşmeleri korunur.
- Günlük döngü dar kapsamlıdır; teslimat kontrollerini projenin CI ve kullanıcı
  gereksinimleri belirler. Belirli framework veya ownership stili dayatılmaz.
- Genel MCP protokolü ve gerçek Rust fixture'ları yayın kapısıdır. Eski istemci
  uyumluluğu ayrı kontrol edilir; tek istemci sürümü ürünün önkoşulu olamaz.
- Yeni sürüm 0.5.0. Tag, checksum, paket ve Registry kimlik kontrolleri korunur.

## İş paketleri ve çıkış ölçütleri

1. **Kaynak bağlamı:** offline/lock'suz ve çok paketli fixture, inheritance,
   UTF-8, pagination, limit, iptal ve path escape testleri.
2. **Yürütme:** liste/derleme/tam test, ignored ve eşleşmeyen test, bin/example,
   argüman geçişi, derleme hatası, nonzero exit, timeout ve cleanup testleri.
3. **MCP entegrasyonu:** tipli giriş/çıkış şemaları, yapılandırma önceliği,
   doğru annotations, client root daraltması ve stdio uçtan uca testleri.
4. **Genelleştirme ve dağıtım:** EN/TR kullanım örnekleri, güncel iş akışları,
   bağımlılık güvenlik düzeltmesi, üç platform CI ve kanonik yayın.
5. **Kanıt:** format, warnings-as-errors Clippy, tüm hedef testleri, MSRV,
   release build, protokol/benchmark smokes, paketleme ve yayın kimliği.

## Sınırlar

MCP genel amaçlı terminal veya model yönlendiricisi değildir. Her crate'in her
platformda kurulabileceği iddia edilmez; ilgili Rust target, linker ve native
bağımlılıklar proje ortamında bulunmalıdır. Kontrollerin uygulanmadığı veya
çıktının kesildiği durumlar açık sonuçlarla raporlanır.
