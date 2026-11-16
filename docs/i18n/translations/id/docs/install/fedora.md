# Pratinjau modem Linkwise untuk Fedora

Linkwise menyediakan modem perangkat lunak dua arah, diagnostik stasiun, dan simulasi lokal. Lihat Station Setup untuk jalur audio dan pengaturan transmisi.

Penginstal pratinjau tidak menyertakan `secure-transfer`. Kompilasi sumber untuk transfer gzip/SHA-256, PKI bersama, obrolan dan surel terlindungi, serta kartu QSL berisi sertifikat. [Panduan PKI](../../../../../../rust/docs/pki.md) menjelaskan konfigurasi dan obrolan hibrida.

## Instal atau perbarui

Jalankan `uname -m` untuk mengetahui arsitektur. Jalankan perintah berikut di direktori unduhan dan ganti nama berkas dengan nama paket yang sesuai.

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF memasang pustaka sistem yang diperlukan. Perbarui dengan `sudo dnf upgrade ./FILE.rpm` dan berkas baru. Simulasi berjalan lokal tanpa radio.

## Atur stasiun

Gunakan `man 8 linkwise-modemd` untuk perintah dan `man 5 linkwise-station` untuk format. Lihat `/usr/share/doc/linkwise-modem/Station-setup.md` untuk memilih perangkat dan membuat `station.json`. Jalankan pemeriksaan sebagai pengguna desktop agar Linkwise dapat mengakses sesi audio.

## Jalankan layanan modem

Untuk stasiun desktop, simpan konfigurasi di `~/.config/linkwise/station.json`, lalu jalankan perintah berikut.

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

Untuk stasiun khusus, simpan konfigurasi dan profil milik administrator di `/etc/linkwise`. Layanan harus dapat membaca berkas dan mengakses langsung masukan serta keluaran ALSA yang diatur. Gunakan satu layanan saja untuk perangkat dan pasangan porta yang sama.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

Jika `bandwidth` diatur, kedua porta modem terbuka pada loopback IPv4. Jika kolom ini dihilangkan, porta perintah menampilkan diagnostik penerimaan JSON. Baca dengan `nc 127.0.0.1 8300`.

### SELinux

Pertahankan SELinux dalam mode enforcing. RPM memasang kebijakan layanan sistem. Setelah membuat atau mengganti konfigurasi, pulihkan label dan verifikasi domain yang aktif.

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

Harapkan `linkwise_modem_exec_t` pada program, `linkwise_modem_etc_t` pada konfigurasi, dan `linkwise_modem_t` pada layanan sistem. Porta TCP 8300 dan 8301 bertipe `linkwise_modem_port_t`. Untuk porta lain, tetapkan tipe tersebut dan selesaikan konflik label. Layanan desktop berjalan di domain pengguna yang menjalankannya.

Uji penegakan SELinux Fedora dan kualifikasi DISA STIG merupakan pemeriksaan terpisah. Kualifikasikan citra OS yang tercakup STIG, varian kebijakan, dan konfigurasi dasar lokasi sebelum penerapan.

## Hapus instalasi

Hentikan perintah Linkwise yang berjalan lalu jalankan `sudo dnf remove linkwise-modem`. Berkas stasiun di direktori pribadi tetap tersedia.
