# Pratinjau modem Linkwise untuk macOS

Linkwise menyediakan modem perangkat lunak dua arah, diagnostik stasiun, dan simulasi lokal. Lihat Station Setup untuk jalur audio dan pengaturan transmisi.

Penginstal pratinjau tidak menyertakan `secure-transfer`. Kompilasi sumber untuk transfer gzip/SHA-256, PKI bersama, obrolan dan surel terlindungi, serta kartu QSL berisi sertifikat. [Panduan PKI](../../../../../../rust/docs/pki.md) menjelaskan konfigurasi dan obrolan hibrida.

## Instal atau perbarui

1. Pada menu Apple → About This Mac, periksa cip atau prosesor. Pilih `macos-arm64.pkg` untuk Apple silicon atau `macos-x86_64.pkg` untuk Intel.
2. Buka paket di Installer dan ikuti petunjuk dengan akun administrator. Buka paket baru dengan cara yang sama untuk memperbarui. Simpan konfigurasi stasiun di direktori pribadi.
3. Buka Terminal dan jalankan perintah berikut.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

Simulasi berjalan secara lokal tanpa radio. Paket pengembangan dapat beredar tanpa tanda tangan. Jika macOS menolak paket, minta build yang telah ditandatangani dan dinotarisasi dari distributor.

## Atur stasiun

Gunakan `man 8 linkwise-modemd` untuk referensi perintah dan `man 5 linkwise-station` untuk format konfigurasi. Buka `/usr/local/share/doc/linkwise-modem/Station-setup.md` untuk memilih perangkat dan membuat `station.json`. Jika perekaman ditolak, periksa System Settings → Privacy & Security → Microphone untuk Terminal.

## Jalankan agen masuk modem

Pada macOS 13 atau yang lebih baru, simpan konfigurasi di `~/Library/Application Support/Linkwise/station.json`, lalu jalankan perintah berikut.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

Jika diminta, beri akses mikrofon dan setujui item latar belakang di System Settings. Gunakan perintah yang sama dengan `status` untuk memeriksa pendaftaran atau `disable` untuk membatalkannya sebelum menghapus instalasi.

## Hapus instalasi

Hentikan perintah Linkwise yang berjalan. Hapus berkas terpasang dan tanda terima paket.

```sh
sudo rm -f /usr/local/bin/linkwise /usr/local/bin/linkwise-modemd \
  /usr/local/share/doc/linkwise-modem/Install.md \
  /usr/local/share/doc/linkwise-modem/LICENSE \
  /usr/local/share/doc/linkwise-modem/THIRD-PARTY-NOTICES.txt \
  /usr/local/share/doc/linkwise-modem/Station-setup.md \
  /usr/local/share/man/man1/linkwise.1 \
  /usr/local/share/man/man8/linkwise-modemd.8 \
  /usr/local/share/man/man5/linkwise-station.5
sudo rm -rf "/Applications/Linkwise Station.app"
sudo pkgutil --forget io.github.8r4n.linkwise.modem
```

Berkas stasiun di direktori pribadi tetap tersedia.
